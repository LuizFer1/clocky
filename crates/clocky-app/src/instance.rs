//! Single instance through a local socket. The first instance listens; a later launch sends
//! its command (`show` by default) and exits, so two copies never run the timers or write the database at once.

use std::io::{BufRead, BufReader, Write};

use interprocess::local_socket::{ListenerOptions, Name, Stream, prelude::*};

use crate::command::Command;

const SOCKET_NAME: &str = "clocky-instance";

/// Windows: a named pipe. Linux: the abstract namespace (nothing left on disk).
#[cfg(not(all(unix, not(target_os = "linux"))))]
fn socket_name() -> std::io::Result<Name<'static>> {
    use interprocess::local_socket::GenericNamespaced;
    SOCKET_NAME.to_ns_name::<GenericNamespaced>()
}

/// macOS and other Unices have no abstract namespace, and `GenericNamespaced` would put a file
/// in `/tmp`. Use a file in the per-user temp dir instead. The file outlives the process (the
/// listener thread is never joined), so `listen` overwrites a stale one.
#[cfg(all(unix, not(target_os = "linux")))]
fn socket_name() -> std::io::Result<Name<'static>> {
    use interprocess::local_socket::GenericFilePath;
    std::env::temp_dir()
        .join(format!("{SOCKET_NAME}.sock"))
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
                for line in BufReader::new(stream).lines().map_while(Result::ok) {
                    let Some(command) = Command::parse(&line) else {
                        tracing::warn!("ignoring unknown instance command: {line}");
                        continue;
                    };
                    if let Err(e) = slint::invoke_from_event_loop(move || on_command(command)) {
                        tracing::warn!("failed to deliver {command:?} to the UI thread: {e}");
                    }
                }
            }
        })?;
    Ok(())
}
