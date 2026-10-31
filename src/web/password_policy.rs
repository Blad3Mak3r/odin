//! What counts as an acceptable password. Length is the main rule; the rest
//! refuses the passwords attackers try first, and anything built from the
//! account's own email or username.

use crate::web::error::BadRequest;

pub const MIN_LEN: usize = 8;
pub const MAX_LEN: usize = 128;

/// Passwords of at least [`MIN_LEN`] characters that appear in every list of
/// the most used ones. Compared in lower case and without punctuation.
const COMMON: &[&str] = &[
    "password",
    "password1",
    "password12",
    "password123",
    "password1234",
    "passw0rd",
    "p4ssw0rd",
    "12345678",
    "123456789",
    "1234567890",
    "12345678910",
    "0123456789",
    "987654321",
    "11111111",
    "00000000",
    "88888888",
    "qwertyui",
    "qwertyuiop",
    "qwerty123",
    "qwerty1234",
    "qwertyuiop123",
    "asdfghjk",
    "asdfghjkl",
    "zxcvbnm1",
    "zxcvbnm123",
    "1q2w3e4r",
    "1q2w3e4r5t",
    "1qaz2wsx",
    "qazwsxedc",
    "zaq12wsx",
    "iloveyou",
    "iloveyou1",
    "sunshine",
    "princess",
    "football",
    "baseball",
    "basketball",
    "superman",
    "batman123",
    "starwars",
    "whatever",
    "trustno1",
    "letmein123",
    "welcome1",
    "welcome123",
    "monkey123",
    "dragon123",
    "master123",
    "shadow123",
    "michael1",
    "abc12345",
    "abcd1234",
    "admin123",
    "admin1234",
    "adminadmin",
    "administrator",
    "changeme",
    "changeme123",
    "default123",
    "secret123",
    "minecraft",
    "valheim",
    "valheim123",
    "odin1234",
    "odinodin",
    "gameserver",
    "mypassword",
    "mypassword1",
    "letmeinnow",
    "freedom1",
    "computer1",
    "internet1",
    "hello1234",
    "hunter123",
    "test1234",
    "testtest",
    "guest123",
    "root1234",
    "toor1234",
    "pa55word",
    "pa55w0rd",
    "q1w2e3r4",
    "q1w2e3r4t5",
];

/// Checks a new password. `identity` lists the account's email and username,
/// which the password must not be built from.
pub fn check(password: &str, identity: &[&str]) -> Result<(), BadRequest> {
    let length = password.chars().count();
    if !(MIN_LEN..=MAX_LEN).contains(&length) {
        return Err(BadRequest(format!(
            "password must be {MIN_LEN}-{MAX_LEN} characters"
        )));
    }
    let lower = password.to_lowercase();
    let squashed: String = lower.chars().filter(|c| c.is_alphanumeric()).collect();

    if COMMON.contains(&lower.as_str()) || COMMON.contains(&squashed.as_str()) {
        return Err(BadRequest(
            "that password is too common; choose something less guessable".into(),
        ));
    }
    let distinct = lower
        .chars()
        .collect::<std::collections::HashSet<_>>()
        .len();
    if distinct <= 2 || is_run(&lower) {
        return Err(BadRequest(
            "that password is too simple: avoid repeats and sequences like 12345678".into(),
        ));
    }
    for name in identity {
        // For an email only the part before `@` identifies the person.
        let name = name.split('@').next().unwrap_or_default().to_lowercase();
        if name.chars().count() >= 4 && lower.contains(&name) {
            return Err(BadRequest(
                "the password must not contain your email or username".into(),
            ));
        }
    }
    Ok(())
}

/// Every character follows the previous one by a step of one, up or down
/// (`abcdefgh`, `87654321`).
fn is_run(text: &str) -> bool {
    let codes: Vec<i64> = text.chars().map(|c| i64::from(u32::from(c))).collect();
    let steps: Vec<i64> = codes.windows(2).map(|pair| pair[1] - pair[0]).collect();
    !steps.is_empty() && (steps.iter().all(|s| *s == 1) || steps.iter().all(|s| *s == -1))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn refused(password: &str, identity: &[&str]) -> String {
        check(password, identity).expect_err(password).0
    }

    #[test]
    fn length_is_enforced() {
        assert!(refused("short1", &[]).contains("8-128"));
        assert!(refused(&"x7".repeat(65), &[]).contains("8-128"));
        assert!(check("tr0ub4dor&3", &[]).is_ok());
        assert!(check("correct-horse-battery-staple", &[]).is_ok());
    }

    #[test]
    fn common_passwords_are_refused_whatever_the_punctuation_or_case() {
        for bad in [
            "password",
            "Password123",
            "PASS-WORD",
            "12345678",
            "Qwerty123",
            "iloveyou",
            "Admin@Admin",
        ] {
            assert!(refused(bad, &[]).contains("too common"), "{bad}");
        }
    }

    #[test]
    fn repeats_and_runs_are_refused() {
        for bad in ["aaaaaaaa", "abababab", "abcdefgh", "87654321", "stuvwxyz"] {
            assert!(refused(bad, &[]).contains("too simple"), "{bad}");
        }
        // Not a run: a step of two, or a break in the middle.
        assert!(check("acegikmo", &[]).is_ok());
        assert!(check("abcd9fgh", &[]).is_ok());
    }

    #[test]
    fn the_account_names_cannot_be_the_password() {
        let who = ["sam.jones@example.com", "SamJones", "x"];
        assert!(refused("sam.jones-2024", &who).contains("email or username"));
        assert!(refused("MY-samjones-99", &who).contains("email or username"));
        // Short names and unrelated passwords are fine.
        assert!(check("blue-Pelican-42", &who).is_ok());
        assert!(check("sam-and-me-2024", &["sam@example.com"]).is_ok());
    }
}
