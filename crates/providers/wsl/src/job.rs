//! Win32 Job Object 封装：保证会话关闭（含 `Drop`）时整棵子进程树被结束。
//!
//! 只有 Windows 实现使用 `unsafe`（`windows-sys` 只暴露 unsafe FFI），因此该模块单独
//! `#[allow(unsafe_code)]`；其他平台是空实现，不涉及 unsafe。

#[cfg(windows)]
#[allow(unsafe_code)]
mod imp {
    use std::ffi::c_void;
    use std::io;
    use std::mem::size_of;
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOBOBJECT_BASIC_LIMIT_INFORMATION, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JobObjectExtendedLimitInformation, SetInformationJobObject, TerminateJobObject,
    };

    /// 设置了 `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` 的 Job Object 句柄。
    ///
    /// 句柄用 `usize` 保存而非 `HANDLE`，让本类型自动满足 `Send`，
    /// 避免再写一条 `unsafe impl Send`。
    #[derive(Debug)]
    pub(crate) struct JobGuard {
        handle: usize,
    }

    impl JobGuard {
        /// 创建匿名 Job Object；失败时返回 `None`（调用方退化为只杀直接子进程）。
        pub(crate) fn create() -> Option<Self> {
            // SAFETY: 两个指针分别表示"使用默认安全属性"和"匿名 Job"，
            // 传空指针是 Win32 明确允许的；返回的句柄由 JobGuard 持有并在 Drop 中关闭。
            let handle = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
            if handle.is_null() {
                tracing::warn!(
                    error = %io::Error::last_os_error(),
                    "创建 Job Object 失败，进程泄漏保护降级"
                );
                return None;
            }

            let limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION {
                BasicLimitInformation: JOBOBJECT_BASIC_LIMIT_INFORMATION {
                    LimitFlags: JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
                    ..Default::default()
                },
                ..Default::default()
            };

            // SAFETY: handle 有效；limits 是栈上的合法结构体，调用期间不会移动，
            // 长度按 size_of 精确传入。
            let written = unsafe {
                SetInformationJobObject(
                    handle,
                    JobObjectExtendedLimitInformation,
                    std::ptr::from_ref(&limits).cast::<c_void>(),
                    size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                )
            };
            if written == 0 {
                tracing::warn!(
                    error = %io::Error::last_os_error(),
                    "设置 Job Object 的 KILL_ON_JOB_CLOSE 失败，进程泄漏保护降级"
                );
                // SAFETY: handle 刚刚由 CreateJobObjectW 创建且未移交，
                // 这里关闭不会造成二次释放。
                unsafe { CloseHandle(handle) };
                return None;
            }

            Some(Self {
                handle: handle as usize,
            })
        }

        /// 把子进程加入 Job。
        ///
        /// `process` 来自 `tokio::process::Child::raw_handle()`，子进程存活期间有效。
        pub(crate) fn assign(&self, process: usize) -> io::Result<()> {
            // SAFETY: self.handle 由本结构持有且有效；process 是存活子进程的句柄，
            // 调用期间由 tokio 的 Child 保证不被关闭。
            let assigned = unsafe { AssignProcessToJobObject(self.raw(), process as HANDLE) };
            if assigned == 0 {
                Err(io::Error::last_os_error())
            } else {
                Ok(())
            }
        }

        /// 立即终止 Job 中的所有进程（超时或主动关闭时使用）。
        pub(crate) fn terminate(&self) {
            // SAFETY: self.handle 由本结构持有且有效；终止 Job 不影响句柄本身。
            unsafe { TerminateJobObject(self.raw(), 1) };
        }

        fn raw(&self) -> HANDLE {
            self.handle as HANDLE
        }
    }

    impl Drop for JobGuard {
        fn drop(&mut self) {
            // SAFETY: 关闭最后一个 Job 句柄会触发 KILL_ON_JOB_CLOSE，
            // 强制结束仍挂在 Job 上的所有进程，避免 wsl.exe 及其子进程泄漏。
            // 该句柄只由本结构持有。
            unsafe { CloseHandle(self.raw()) };
        }
    }
}

#[cfg(not(windows))]
mod imp {
    use std::io;

    /// 非 Windows 平台没有 Job Object，仅保留同名 API 让上层代码保持平台无关。
    #[derive(Debug)]
    pub(crate) struct JobGuard;

    impl JobGuard {
        pub(crate) fn create() -> Option<Self> {
            None
        }

        pub(crate) fn assign(&self, _process: usize) -> io::Result<()> {
            Ok(())
        }

        pub(crate) fn terminate(&self) {}
    }
}

pub(crate) use imp::JobGuard;
