//! How `kairos login --email` gets the password (COLLIERY-T-0213).
//!
//! There are two sources and no others:
//!
//! - **The terminal**, when standard input is one. The CLI asks, with the
//!   echo off, so the password is not on the screen and not in the
//!   scrollback.
//! - **Standard input**, when it is a pipe or a file: one line. This is for
//!   a script or a secret manager:
//!   `printf '%s' "$PW" | kairos login --url <URL> --email <EMAIL>`.
//!
//! The password is never an argument, because an argument is visible in the
//! process list and stays in the shell history. It is never an environment
//! variable, because the environment of a process is readable by other
//! processes of the same user and is inherited by every child.
//!
//! The decision between the two sources, and the reading of the line, are in
//! [`read_from`], which takes both sources as parameters. The terminal prompt
//! itself is one call into `rpassword`, in [`read`]. So the unit tests cover
//! everything but that one call, and a live test covers the call through a
//! pseudo-terminal.

use std::io::{BufRead, IsTerminal};

use kairos_client::types_auth::Secret;

use crate::error::CliError;

/// Read the password for `email` from the terminal or from standard input.
pub fn read(email: &str) -> Result<Secret, CliError> {
    let stdin = std::io::stdin();
    let is_terminal = stdin.is_terminal();
    read_from(
        is_terminal,
        &format!("Password for {email}: "),
        |prompt| rpassword::prompt_password(prompt),
        &mut stdin.lock(),
    )
}

/// [`read`], with its two sources as parameters.
///
/// `prompt_hidden` asks on the terminal with the echo off. `piped` is
/// standard input when it is not a terminal; it is not read when
/// `is_terminal` is true.
pub fn read_from<P>(
    is_terminal: bool,
    prompt: &str,
    prompt_hidden: P,
    piped: &mut dyn BufRead,
) -> Result<Secret, CliError>
where
    P: FnOnce(&str) -> std::io::Result<String>,
{
    let password = if is_terminal {
        prompt_hidden(prompt).map_err(|err| {
            CliError::Failure(format!(
                "The CLI cannot read the password from the terminal: {err}.\n\
                 Run the command in a terminal, or send the password on standard input."
            ))
        })?
    } else {
        let mut line = String::new();
        piped.read_line(&mut line).map_err(|err| {
            CliError::Failure(format!(
                "The CLI cannot read the password from standard input: {err}."
            ))
        })?;
        line
    };
    // Only the end of the line. A space at the start or at the end is a part
    // of the password: the server does not trim it, so the CLI must not.
    let password = password
        .strip_suffix('\n')
        .map(|rest| rest.strip_suffix('\r').unwrap_or(rest))
        .unwrap_or(&password);
    if password.is_empty() {
        return Err(CliError::Failure(
            "The command got no password.\n\
             Type the password at the prompt, or send it on standard input."
                .to_string(),
        ));
    }
    Ok(Secret::new(password))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::EXIT_FAILURE;
    use std::io::Cursor;

    fn no_terminal(_: &str) -> std::io::Result<String> {
        panic!("the terminal must not be asked when standard input is a pipe")
    }

    /// A reader that fails the test if the CLI reads it.
    struct Untouched;
    impl std::io::Read for Untouched {
        fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
            panic!("standard input must not be read when it is a terminal")
        }
    }

    fn piped(input: &str) -> Result<Secret, CliError> {
        read_from(false, "Password: ", no_terminal, &mut Cursor::new(input))
    }

    /// One line from a pipe, with or without the end of the line.
    #[test]
    fn the_piped_password_is_one_line() {
        assert_eq!(
            piped("s3cret pass").expect("no newline").expose(),
            "s3cret pass"
        );
        assert_eq!(
            piped("s3cret pass\n").expect("newline").expose(),
            "s3cret pass"
        );
        assert_eq!(
            piped("s3cret pass\r\n").expect("crlf").expose(),
            "s3cret pass"
        );
        // Only the first line is the password.
        assert_eq!(
            piped("first\nsecond\n").expect("two lines").expose(),
            "first"
        );
        // Spaces are a part of the password.
        assert_eq!(
            piped("  padded  \n").expect("spaces").expose(),
            "  padded  "
        );
        // One end of line is removed, and not more.
        assert_eq!(piped("tail\r\r\n").expect("cr").expose(), "tail\r");
    }

    /// No password is a failure with an instruction, not a request with an
    /// empty password, which would count against the throttle.
    #[test]
    fn an_empty_password_is_refused() {
        for input in ["", "\n", "\r\n"] {
            let err = piped(input).expect_err("empty");
            assert_eq!(err.exit_code(), EXIT_FAILURE);
            assert!(
                err.to_string().contains("The command got no password"),
                "{err}"
            );
        }
    }

    /// With standard input a terminal, the CLI asks on the terminal, with
    /// the prompt that names the account, and does not read standard input.
    #[test]
    fn a_terminal_is_asked_and_stdin_is_not_read() {
        let mut asked = None;
        let password = read_from(
            true,
            "Password for ada@example.test: ",
            |prompt| {
                asked = Some(prompt.to_string());
                Ok("typed at the prompt".to_string())
            },
            &mut std::io::BufReader::new(Untouched),
        )
        .expect("password");
        assert_eq!(password.expose(), "typed at the prompt");
        assert_eq!(asked.as_deref(), Some("Password for ada@example.test: "));
    }

    /// A terminal that cannot be read is a failure that says what to do.
    #[test]
    fn a_terminal_that_cannot_be_read_is_a_failure() {
        let err = read_from(
            true,
            "Password: ",
            |_| Err(std::io::Error::other("no tty")),
            &mut std::io::BufReader::new(Untouched),
        )
        .expect_err("no terminal");
        assert_eq!(err.exit_code(), EXIT_FAILURE);
        assert!(err.to_string().contains("standard input"), "{err}");
    }

    /// The value that comes back does not print itself.
    #[test]
    fn the_password_has_no_debug_output() {
        let password = piped("s3cret pass\n").expect("password");
        assert!(!format!("{password:?}").contains("s3cret"));
    }
}
