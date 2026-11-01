//! Windows 服务胶合层(SCM 手写,零第三方依赖)。
//!
//! 服务身份运行(main 检测 --service 参数,由 Task 7 的安装脚本 sc create
//! 带上):run_service 经 StartServiceCtrlDispatcherW 把主线程交给 SCM 分发
//! (阻塞至服务停止),service_proc 注册控制处理器并报 START_PENDING →
//! 工作线程跑 daemon 主逻辑(既有 daemon_main,tokio runtime 自建)→ 报
//! RUNNING → SCM 下发 STOP/SHUTDOWN 时控制回调置既有停止通道(Task 5 的
//! request_shutdown)→ 主逻辑走既有优雅收尾后退出 → 报 STOPPED。
//! 控制台直跑(无 --service)不经过本模块,行为与 mac/linux 一致。

use windows_sys::Win32::Foundation::{
    GetLastError, ERROR_FAILED_SERVICE_CONTROLLER_CONNECT, ERROR_SERVICE_SPECIFIC_ERROR, NO_ERROR,
};
use windows_sys::Win32::System::Services::*;

/// SCM 分发表里的服务名(Task 7 的 sc create 服务名 planetportd 与之配套)
const SERVICE_NAME: &str = "planetportd";

/// 工作线程 panic 时上抛的服务特定退出码(取值自定,SCM 只要求非 0;
/// dwWin32ExitCode=ERROR_SERVICE_SPECIFIC_ERROR 时 SCM 视为 failure,
/// Task 7 的 sc failure 自拉起策略才生效)
const SPECIFIC_EXIT_PANIC: u32 = 101;

/// 报给 SCM 的等待上限(START_PENDING/STOP_PENDING 的 WaitHint):daemon
/// 收尾(逐 planet 拆设备 + registry 落盘)有界,30s 足够;超窗 SCM 判定
/// 服务挂起并强制结束,与「收尾必须有界」的既有设计一致
const WAIT_HINT_MS: u32 = 30000;

/// daemon 主逻辑由 run_service 存入,service_proc 取出丢工作线程跑。
/// service_proc 是 extern "system" 回调,签名固定无法经参数携带闭包,
/// 只能经静态槽传递(每次进程至多一个服务入口)。Mutex 而 非 OnceLock:
/// 静态项要求 Sync,FnOnce 闭包盒子只有 Send
static MAIN_BODY: std::sync::Mutex<Option<Box<dyn FnOnce() + Send>>> =
    std::sync::Mutex::new(None);

/// 服务状态句柄:RegisterServiceCtrlHandlerExW 的返回值。控制回调与
/// service_proc 分属两个线程,都靠它报状态;null 即尚未注册成功,报状态
/// 是 no-op(注册失败时 SCM 很快终止进程,无需额外处理)
static STATUS_HANDLE: std::sync::atomic::AtomicPtr<core::ffi::c_void> =
    std::sync::atomic::AtomicPtr::new(std::ptr::null_mut());

/// 上报服务状态。dwServiceType/dwControlsAccepted/dwCheckPoint 取 brief
/// 定型值;STOPPED 时清零 ControlsAccepted(SCM 约定:终态不再接受控制码)。
/// specific 仅在 exit_code == ERROR_SERVICE_SPECIFIC_ERROR 时有效(Win32
/// 语义),其余路径传 0
fn report_status(state: u32, exit_code: u32, specific: u32) {
    let handle = STATUS_HANDLE.load(std::sync::atomic::Ordering::SeqCst);
    if handle.is_null() {
        return;
    }
    let status = SERVICE_STATUS {
        dwServiceType: SERVICE_WIN32_OWN_PROCESS,
        dwCurrentState: state,
        dwControlsAccepted: if state == SERVICE_STOPPED {
            0
        } else {
            SERVICE_ACCEPT_STOP | SERVICE_ACCEPT_SHUTDOWN
        },
        dwWin32ExitCode: exit_code,
        dwServiceSpecificExitCode: specific,
        dwCheckPoint: 0,
        dwWaitHint: WAIT_HINT_MS,
    };
    unsafe { SetServiceStatus(handle, &status) };
}

/// SCM 控制回调(SCM 自己的线程,禁止阻塞):STOP/SHUTDOWN 只做两件事——
/// 置既有停止通道触发主逻辑优雅收尾、立即应答 STOP_PENDING(30s 窗口由
/// 工作线程的收尾消耗,结束后 service_proc 报 STOPPED)。其余控制码
/// (INTERROGATE/PARAMCHANGE 等)未申请,直接返回 NO_ERROR。
/// 返回值是 Win32_ERROR:NO_ERROR 即已处理。
unsafe extern "system" fn handler_ctrl(
    dw_control: u32,
    _dw_event_type: u32,
    _lp_event_data: *mut core::ffi::c_void,
    _lp_context: *mut core::ffi::c_void,
) -> u32 {
    if matches!(dw_control, SERVICE_CONTROL_STOP | SERVICE_CONTROL_SHUTDOWN) {
        crate::request_shutdown();
        report_status(SERVICE_STOP_PENDING, NO_ERROR, 0);
    }
    NO_ERROR
}

/// 服务主函数:main 在 --service 时调用;阻塞至服务停止,返回进程退出码。
/// 返回 0 = 服务正常走完生命周期(或 SCM 缺席回退控制台模式跑完);非 0 =
/// SCM 分发失败的 Win32 错误码。
pub fn run_service<F: FnOnce() + Send + 'static>(main_body: F) -> u32 {
    let name: Vec<u16> = format!("{SERVICE_NAME}\0").encode_utf16().collect();
    let mut table = [
        SERVICE_TABLE_ENTRYW {
            lpServiceName: name.as_ptr() as *mut u16,
            lpServiceProc: Some(service_proc),
        },
        SERVICE_TABLE_ENTRYW {
            lpServiceName: std::ptr::null_mut(),
            lpServiceProc: None,
        },
    ];
    // 主逻辑先入静态槽:service_proc 是 SCM 回调,只能从槽里取
    *MAIN_BODY.lock().unwrap_or_else(|e| e.into_inner()) = Some(Box::new(main_body));
    let ok = unsafe { StartServiceCtrlDispatcherW(table.as_mut_ptr()) };
    if ok != 0 {
        return 0;
    }
    let err = unsafe { GetLastError() };
    if err == ERROR_FAILED_SERVICE_CONTROLLER_CONNECT {
        // SCM 缺席(--service 被控制台直跑,如开发调试):回退控制台模式,
        // 与无 --service 的直跑完全同路径。仅此错误码可回退——它是
        // 「进程未被 SCM 启动」的唯一信号;其余错误码(权限/表结构)是
        // 服务语境下的真实故障,必须上抛退出码
        eprintln!("[service] 未在服务控制管理器上下文中启动,回退控制台模式");
        if let Some(body) = MAIN_BODY.lock().unwrap_or_else(|e| e.into_inner()).take() {
            body();
        }
        return 0;
    }
    err
}

/// 服务入口:SCM 在新线程调起(每服务一次)。注册控制处理器 → START_PENDING
/// → 工作线程跑主逻辑 → RUNNING → 等工作线程退出(优雅收尾在主逻辑内完成,
/// 由 STOP 控制回调经 request_shutdown 触发)→ STOPPED。
unsafe extern "system" fn service_proc(_argc: u32, _argv: *mut *mut u16) {
    let name: Vec<u16> = format!("{SERVICE_NAME}\0").encode_utf16().collect();
    let status_handle = unsafe {
        RegisterServiceCtrlHandlerExW(name.as_ptr(), Some(handler_ctrl), std::ptr::null())
    };
    if status_handle.is_null() {
        // 句柄拿不到就无法上报任何状态;直接返回让 SCM 按启动超时收尾
        return;
    }
    STATUS_HANDLE.store(status_handle, std::sync::atomic::Ordering::SeqCst);

    report_status(SERVICE_START_PENDING, NO_ERROR, 0);
    // 主逻辑在工作线程跑:daemon_main 全程 async(tokio runtime 需在块内
    // 构建),而本线程是 SCM 回调线程,不能被 tokio 长期占用
    let worker = MAIN_BODY
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take()
        .map(std::thread::spawn);
    report_status(SERVICE_RUNNING, NO_ERROR, 0);
    // join 不设超时但有界:收尾本身有界(既有 shutdown 设计),且控制回调
    // 已在 STOP 时报 STOP_PENDING 占住 30s WaitHint 窗口。Err 即主逻辑
    // panic 穿透:panic 以服务特定错误上报,令 sc failure 拉起生效——若
    // 仍报 NO_ERROR,SCM 视角是干净停止,自拉起策略不会触发
    let panicked = if let Some(worker) = worker {
        worker.join().is_err()
    } else {
        false
    };
    if panicked {
        report_status(SERVICE_STOPPED, ERROR_SERVICE_SPECIFIC_ERROR, SPECIFIC_EXIT_PANIC);
    } else {
        report_status(SERVICE_STOPPED, NO_ERROR, 0);
    }
}
