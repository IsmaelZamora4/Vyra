use serde::{Deserialize, Serialize};
use std::io::{BufRead, Read, Write};

pub const VERSION: u8 = 3;
pub const MAX_FRAME: usize = 2048;
pub const MAX_MS: u64 = 10_000;
pub const PRECONFIRM_MS: u64 = 60_000;

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
pub enum Command {
    Hello {
        version: u8,
        session: String,
        parent_pid: u32,
    },
    Apply {
        session: String,
    },
    Arm {
        session: String,
    },
    Restore {
        session: String,
    },
    Cancel {
        session: String,
    },
    #[cfg(debug_assertions)]
    Advance {
        session: String,
        elapsed_ms: u64,
    },
}
impl Command {
    pub fn session(&self) -> &str {
        match self {
            Self::Hello { session, .. }
            | Self::Apply { session }
            | Self::Arm { session }
            | Self::Restore { session }
            | Self::Cancel { session } => session,
            #[cfg(debug_assertions)]
            Self::Advance { session, .. } => session,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
pub enum Event {
    Ready {
        session: String,
        max_ms: u64,
        helper_initialized: bool,
        helper_read_accepted: bool,
        journal_durable: bool,
        same_windows_session: bool,
    },
    Armed {
        session: String,
        max_ms: u64,
    },
    Applied {
        session: String,
        accepted: bool,
        verified: bool,
    },
    Finished {
        session: String,
        restored: bool,
        accepted: Option<bool>,
        verified: bool,
        reason: String,
    },
}

pub fn read_frame<T: for<'a> Deserialize<'a>>(
    reader: &mut impl BufRead,
) -> Result<Option<T>, String> {
    let mut bytes = Vec::new();
    let n = reader
        .take((MAX_FRAME + 1) as u64)
        .read_until(b'\n', &mut bytes)
        .map_err(|e| e.to_string())?;
    if n == 0 {
        return Ok(None);
    }
    if n > MAX_FRAME || bytes.last() != Some(&b'\n') {
        return Err("Trama IPC truncada o demasiado grande.".into());
    }
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|e| format!("IPC inválido: {e}"))
}
pub fn send<T: Serialize>(writer: &mut impl Write, value: &T) -> Result<(), String> {
    let mut bytes = serde_json::to_vec(value).map_err(|e| e.to_string())?;
    bytes.push(b'\n');
    if bytes.len() > MAX_FRAME {
        return Err("Trama IPC demasiado grande.".into());
    }
    writer
        .write_all(&bytes)
        .and_then(|_| writer.flush())
        .map_err(|e| e.to_string())
}
pub fn valid_session(session: &str) -> bool {
    session.len() == 64
        && session
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_oversized_partial_unknown_and_unsafe_messages() {
        for bytes in [
            vec![b'x'; MAX_FRAME + 1],
            b"{\"type\":\"restore\"}".to_vec(),
            b"{\"type\":\"write\",\"matrix\":[]}\n".to_vec(),
            b"{\"type\":\"apply\",\"session\":\"x\",\"path\":\"C:\\\\evil.exe\"}\n".to_vec(),
        ] {
            assert!(read_frame::<Command>(&mut bytes.as_slice()).is_err());
        }
    }
}
