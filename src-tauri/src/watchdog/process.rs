use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ParentIdentity {
    pub pid: u32,
    pub created: u64,
}

#[cfg(target_os = "windows")]
pub struct ParentProcess(windows::Win32::Foundation::HANDLE);

#[cfg(not(target_os = "windows"))]
pub struct ParentProcess;
#[cfg(not(target_os = "windows"))]
impl ParentProcess {
    pub fn open(_: u32) -> Result<Self, String> {
        Err("Se requiere Windows para observar al padre.".into())
    }
    pub fn identity(&self, _: u32) -> Result<ParentIdentity, String> {
        Err("Se requiere Windows.".into())
    }
    pub fn alive(&self) -> bool {
        false
    }
    pub fn shares_windows_session(&self, _: u32) -> Result<bool, String> {
        Err("Se requiere Windows para comprobar la sesión del proceso.".into())
    }
}
#[cfg(target_os = "windows")]
impl ParentProcess {
    pub fn open(pid: u32) -> Result<Self, String> {
        use windows::Win32::System::Threading::{
            OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
        };
        unsafe {
            OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
                false,
                pid,
            )
        }
        .map(Self)
        .map_err(|e| format!("Proceso padre no accesible: {e}"))
    }
    pub fn identity(&self, pid: u32) -> Result<ParentIdentity, String> {
        use windows::Win32::{Foundation::FILETIME, System::Threading::GetProcessTimes};
        let (mut created, mut exited, mut kernel, mut user) = (
            FILETIME::default(),
            FILETIME::default(),
            FILETIME::default(),
            FILETIME::default(),
        );
        unsafe { GetProcessTimes(self.0, &mut created, &mut exited, &mut kernel, &mut user) }
            .map_err(|e| e.to_string())?;
        Ok(ParentIdentity {
            pid,
            created: ((created.dwHighDateTime as u64) << 32) | created.dwLowDateTime as u64,
        })
    }
    pub fn alive(&self) -> bool {
        unsafe {
            windows::Win32::System::Threading::WaitForSingleObject(self.0, 0)
                == windows::Win32::Foundation::WAIT_TIMEOUT
        }
    }
    pub fn shares_windows_session(&self, parent_pid: u32) -> Result<bool, String> {
        use windows::Win32::System::RemoteDesktop::ProcessIdToSessionId;
        let mut parent_session = 0;
        let mut helper_session = 0;
        unsafe {
            ProcessIdToSessionId(parent_pid, &mut parent_session).map_err(|error| {
                format!("No se pudo consultar la sesión del proceso principal: {error}")
            })?;
            ProcessIdToSessionId(std::process::id(), &mut helper_session)
                .map_err(|error| format!("No se pudo consultar la sesión del watchdog: {error}"))?;
        }
        Ok(parent_session != 0 && parent_session == helper_session)
    }
}
#[cfg(target_os = "windows")]
impl Drop for ParentProcess {
    fn drop(&mut self) {
        unsafe {
            let _ = windows::Win32::Foundation::CloseHandle(self.0);
        }
    }
}
impl ParentIdentity {
    pub fn current() -> Result<Self, String> {
        #[cfg(target_os = "windows")]
        {
            let pid = std::process::id();
            ParentProcess::open(pid)?.identity(pid)
        }
        #[cfg(not(target_os = "windows"))]
        {
            Err("Watchdog nativo solo disponible en Windows.".into())
        }
    }
}
