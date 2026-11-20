//! Windows guard acquired before Tauri creates services or reads journals.
//! https://learn.microsoft.com/windows/win32/api/synchapi/nf-synchapi-createmutexw
#[cfg(target_os = "windows")]
pub struct InstanceGuard(windows::Win32::Foundation::HANDLE);

#[cfg(not(target_os = "windows"))]
pub struct InstanceGuard;
#[cfg(not(target_os = "windows"))]
impl InstanceGuard {
    pub fn acquire(_: &str) -> Result<Option<Self>, String> {
        Err("El watchdog de esta fase requiere Windows.".into())
    }
}

#[cfg(target_os = "windows")]
impl InstanceGuard {
    pub fn acquire(name: &str) -> Result<Option<Self>, String> {
        use windows::Win32::{
            Foundation::{
                CloseHandle, ERROR_ALREADY_EXISTS, ERROR_SUCCESS, GetLastError, SetLastError,
            },
            System::Threading::CreateMutexW,
        };
        let name: Vec<u16> = name.encode_utf16().chain([0]).collect();
        unsafe {
            // The non-inheritable handle, rather than thread ownership, holds the lease.
            SetLastError(ERROR_SUCCESS);
            let handle = CreateMutexW(None, false, windows::core::PCWSTR(name.as_ptr()))
                .map_err(|error| format!("No se pudo asegurar instancia única: {error}"))?;
            let existed = GetLastError() == ERROR_ALREADY_EXISTS;
            if existed {
                let _ = CloseHandle(handle);
                Ok(None)
            } else {
                Ok(Some(Self(handle)))
            }
        }
    }
}

#[cfg(target_os = "windows")]
impl Drop for InstanceGuard {
    fn drop(&mut self) {
        unsafe {
            let _ = windows::Win32::Foundation::CloseHandle(self.0);
        }
    }
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use super::*;

    #[test]
    fn single_instance_child() {
        if let Ok(name) = std::env::var("VYRA_TEST_INSTANCE_NAME") {
            let acquired = InstanceGuard::acquire(&name).unwrap().is_some();
            assert_eq!(acquired, std::env::var("VYRA_TEST_INSTANCE_FREE").is_ok());
        }
    }

    #[test]
    fn rejects_second_process_and_releases_on_exit() {
        let name = format!("Local\\VYRA.test.{}", std::process::id());
        let guard = InstanceGuard::acquire(&name).unwrap().unwrap();
        let run_child = |free: bool| {
            let mut child = std::process::Command::new(std::env::current_exe().unwrap());
            child.args(["--exact", "single_instance::tests::single_instance_child"]);
            child.env("VYRA_TEST_INSTANCE_NAME", &name);
            child.env_remove("VYRA_TEST_INSTANCE_FREE");
            if free {
                child.env("VYRA_TEST_INSTANCE_FREE", "1");
            }
            assert!(child.status().unwrap().success());
        };
        run_child(false);
        drop(guard);
        run_child(true);
    }
}
