//! Commands a second launch sends to the running instance (`clocky --send <command>`).

/// A command for the running instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    /// Bring the manage window to the front (what a plain second launch sends).
    Show,
    /// Quit the app, as the tray's "Sair".
    Quit,
}

impl Command {
    pub fn parse(text: &str) -> Option<Self> {
        match text.trim() {
            "show" => Some(Self::Show),
            "quit" => Some(Self::Quit),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Show => "show",
            Self::Quit => "quit",
        }
    }
}

/// What a launch asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Launch {
    /// `clocky`: start, or show the running instance.
    Start,
    /// `clocky --send <command>`: only deliver a command to the running instance.
    Send(Command),
}

impl Launch {
    /// The command delivered to an instance that is already running.
    pub fn command(self) -> Command {
        match self {
            Self::Start => Command::Show,
            Self::Send(command) => command,
        }
    }
}

/// Parses the arguments after the program name.
pub fn parse_args(args: &[String]) -> anyhow::Result<Launch> {
    match args {
        [] => Ok(Launch::Start),
        [flag, command] if flag == "--send" => Command::parse(command)
            .map(Launch::Send)
            .ok_or_else(|| anyhow::anyhow!("unknown command: {command}")),
        _ => anyhow::bail!("usage: clocky [--send <command>]"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn no_arguments_starts_the_app() {
        assert_eq!(parse_args(&[]).unwrap(), Launch::Start);
        assert_eq!(Launch::Start.command(), Command::Show);
    }

    #[test]
    fn send_delivers_a_command() {
        let launch = parse_args(&args(&["--send", "quit"])).unwrap();
        assert_eq!(launch, Launch::Send(Command::Quit));
        assert_eq!(launch.command(), Command::Quit);
    }

    #[test]
    fn unknown_command_is_an_error() {
        assert!(parse_args(&args(&["--send", "dance"])).is_err());
        assert!(parse_args(&args(&["--bogus"])).is_err());
    }

    #[test]
    fn commands_roundtrip_through_text() {
        for command in [Command::Show, Command::Quit] {
            assert_eq!(Command::parse(command.as_str()), Some(command));
        }
        assert_eq!(Command::parse(" show\n"), Some(Command::Show));
    }
}
