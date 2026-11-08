//! Single instance through a local socket. The first instance listens; a later launch sends
//! its command (`show` by default) and exits, so two copies never run the timers or write the database at once.
//!
//! The socket name includes a hash of the user's config dir: each user of the machine has their
//! own instance, and nobody else's launch reaches it by accident.

use std::io::{BufRead, BufReader, Read, Write};
use std::time::Duration;

use interprocess::local_socket::{ListenerOptions, Name, Stream, prelude::*};

use crate::command::Command;

/// A client gets this long to send its commands (where the OS supports read timeouts)...
const READ_TIMEOUT: Duration = Duration::from_secs(2);
/// ...and at most this many bytes (commands are a few bytes each).
const MAX_COMMAND_BYTES: u64 = 256;

/// `clocky-<FNV-1a 64 of user_key, in hex>`. FNV-1a is fixed, unlike `DefaultHasher`, so every
/// Clocky version computes the same name.
pub fn socket_id(user_key: &str) -> String {
    let hash = user_key
        .bytes()
        .fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
        });
    format!("clocky-{hash:016x}")
}

/// Identifies the current user: the config dir path (it contains the home dir), else the name.
fn user_key() -> String {
    crate::paths::config_dir()
        .map(|dir| dir.display().to_string())
        .or_else(|| std::env::var("USERNAME").ok())
        .or_else(|| std::env::var("USER").ok())
        .unwrap_or_default()
}

/// Windows: a named pipe. Linux: the abstract namespace (nothing left on disk).
#[cfg(not(all(unix, not(target_os = "linux"))))]
fn socket_name() -> std::io::Result<Name<'static>> {
    use interprocess::local_socket::GenericNamespaced;
    socket_id(&user_key()).to_ns_name::<GenericNamespaced>()
}

/// macOS and other Unices have no abstract namespace, and `GenericNamespaced` would put a file
/// in `/tmp`. Use a file in the per-user temp dir instead. The file outlives the process (the
/// listener thread is never joined), so `listen` overwrites a stale one.
#[cfg(all(unix, not(target_os = "linux")))]
fn socket_name() -> std::io::Result<Name<'static>> {
    use interprocess::local_socket::GenericFilePath;
    std::env::temp_dir()
        .join(format!("{}.sock", socket_id(&user_key())))
        .to_fs_name::<GenericFilePath>()
}

/// Tries to deliver `command` to a running instance. `true` = delivered (this launch exits).
pub fn forward(command: Command) -> bool {
    let Ok(name) = socket_name() else {
        return false;
    };
    let Ok(mut stream) = Stream::connect(name) else {
        return false;
    };
    stream
        .write_all(format!("{}\n", command.as_str()).as_bytes())
        .is_ok()
}

/// Becomes the main instance: a thread accepts connections and hands each command to
/// `on_command` on the UI thread.
///
/// Call it only after [`forward`] failed: nobody answers on the name, so a leftover socket file
/// (macOS, after the app quit) is overwritten instead of failing with `AddrInUse`. Named pipes
/// cannot be overwritten, so on Windows this changes nothing.
pub fn listen(on_command: fn(Command)) -> anyhow::Result<()> {
    let listener = ListenerOptions::new()
        .name(socket_name()?)
        .try_overwrite(true)
        .create_sync()?;
    std::thread::Builder::new()
        .name("instance".into())
        .spawn(move || {
            for stream in listener.incoming().filter_map(Result::ok) {
                serve(stream, on_command);
            }
        })?;
    Ok(())
}

/// Serves one client on its own thread, so a client that never sends nor closes cannot block
/// the next launches.
fn serve(stream: Stream, on_command: fn(Command)) {
    let spawned = std::thread::Builder::new()
        .name("instance-client".into())
        .spawn(move || {
            // Unix sockets support it; Windows named pipes do not, and the thread is enough.
            if let Err(e) = stream.set_recv_timeout(Some(READ_TIMEOUT)) {
                tracing::debug!("no read timeout on the instance socket: {e}");
            }
            for command in read_commands(stream) {
                if let Err(e) = slint::invoke_from_event_loop(move || on_command(command)) {
                    tracing::warn!("failed to deliver {command:?} to the UI thread: {e}");
                }
            }
        });
    if let Err(e) = spawned {
        tracing::warn!("failed to start an instance client thread: {e}");
    }
}

/// Reads the commands of one client, one per line, up to [`MAX_COMMAND_BYTES`].
fn read_commands(reader: impl Read) -> Vec<Command> {
    let mut commands = Vec::new();
    for line in BufReader::new(reader.take(MAX_COMMAND_BYTES))
        .lines()
        .map_while(Result::ok)
    {
        if let Some(command) = Command::parse(&line) {
            commands.push(command);
        } else {
            tracing::warn!("ignoring unknown instance command: {line}");
        }
    }
    commands
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn socket_id_is_stable_per_user() {
        let a = socket_id(r"C:\Users\ana\AppData\Roaming\Clocky\config");
        assert_eq!(a, socket_id(r"C:\Users\ana\AppData\Roaming\Clocky\config"));
        assert!(a.starts_with("clocky-"), "{a}");
        assert_eq!(a.len(), "clocky-".len() + 16);
    }

    #[test]
    fn socket_id_differs_between_users() {
        assert_ne!(
            socket_id("/home/ana/.config/clocky"),
            socket_id("/home/bob/.config/clocky")
        );
    }

    #[test]
    fn socket_id_matches_known_fnv1a_value() {
        // FNV-1a 64 of "a" is 0xaf63dc4c8601ec8c: the hash must not change between versions,
        // or an old and a new Clocky would not find each other.
        assert_eq!(socket_id("a"), "clocky-af63dc4c8601ec8c");
    }

    #[test]
    fn reads_every_command_line() {
        let commands = read_commands(&b"show\nquit\n"[..]);
        assert_eq!(commands, [Command::Show, Command::Quit]);
    }

    #[test]
    fn skips_unknown_lines() {
        assert_eq!(read_commands(&b"dance\nshow\n"[..]), [Command::Show]);
    }

    #[test]
    fn reads_at_most_the_byte_limit() {
        let mut flood = "show\n".repeat(1000).into_bytes();
        flood.extend_from_slice(b"quit\n");
        let commands = read_commands(&flood[..]);
        assert!(commands.len() <= 64, "{}", commands.len());
        assert!(!commands.contains(&Command::Quit));
    }
}
