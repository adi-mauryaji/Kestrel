//! The mail password, in the OS keyring only (blueprint §6): never in a file or a log. Not tried in CI:
//! a CI machine has no keyring, and a test here would ask the person's.

const SERVICE: &str = "kestrel";
const USER: &str = "gmail-app-password";

fn entry() -> Result<keyring::Entry, String> {
    keyring::Entry::new(SERVICE, USER).map_err(|e| format!("no keyring here: {e}"))
}

pub fn get() -> Result<Option<String>, String> {
    match entry()?.get_password() {
        Ok(p) => Ok(Some(p)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(format!("the keyring: {e}")),
    }
}

pub fn set(value: &str) -> Result<(), String> {
    entry()?
        .set_password(value)
        .map_err(|e| format!("the keyring: {e}"))
}

pub fn delete() -> Result<(), String> {
    match entry()?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(format!("the keyring: {e}")),
    }
}
