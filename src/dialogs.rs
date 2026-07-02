//! Native MessageBox wrappers. Used where a full Slint screen would be
//! overkill (headless uninstall, mid-update confirmation).

use windows::core::PCWSTR;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{
    MessageBoxW, IDYES, MB_ICONERROR, MB_ICONWARNING, MB_OK, MB_SETFOREGROUND, MB_SYSTEMMODAL,
    MB_YESNO,
};

pub fn yes_no(title: &str, body: &str) -> bool {
    let title_w = to_wide(title);
    let body_w = to_wide(body);
    unsafe {
        let result = MessageBoxW(
            HWND::default(),
            PCWSTR(body_w.as_ptr()),
            PCWSTR(title_w.as_ptr()),
            MB_YESNO | MB_ICONWARNING | MB_SETFOREGROUND | MB_SYSTEMMODAL,
        );
        result == IDYES
    }
}

pub fn error(body: &str) {
    let title_w = to_wide("Codex launcher");
    let body_w = to_wide(body);
    unsafe {
        let _ = MessageBoxW(
            HWND::default(),
            PCWSTR(body_w.as_ptr()),
            PCWSTR(title_w.as_ptr()),
            MB_OK | MB_ICONERROR | MB_SETFOREGROUND | MB_SYSTEMMODAL,
        );
    }
}

fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Result of a `two_button_choice` prompt. `Cancelled` means the user
/// dismissed the dialog (Esc / X) — callers must treat this as "no
/// affirmative choice" and pick a safe default, not silently fall through
/// to `Secondary`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialogChoice {
    Primary,
    Secondary,
    Cancelled,
}

/// Two-button TaskDialog with custom button labels (Vista+ API, lets us
/// avoid the OS-locked OK/Cancel/Yes/No labels). `button1` is selected by
/// default. Returns `Primary`/`Secondary` for explicit clicks, `Cancelled`
/// for dialog dismissal.
pub fn two_button_choice(
    title: &str,
    main_instruction: &str,
    body: &str,
    button1: &str,
    button2: &str,
) -> DialogChoice {
    use windows::core::{PCWSTR, PWSTR};
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::Controls::{
        TaskDialogIndirect, TASKDIALOGCONFIG, TASKDIALOGCONFIG_0, TASKDIALOGCONFIG_1,
        TASKDIALOG_BUTTON, TASKDIALOG_FLAGS, TDF_ALLOW_DIALOG_CANCELLATION, TD_WARNING_ICON,
    };

    let title_w = to_wide(title);
    let instr_w = to_wide(main_instruction);
    let body_w = to_wide(body);
    let b1_w = to_wide(button1);
    let b2_w = to_wide(button2);

    const ID_BTN1: i32 = 1001;
    const ID_BTN2: i32 = 1002;

    let buttons = [
        TASKDIALOG_BUTTON {
            nButtonID: ID_BTN1,
            pszButtonText: PCWSTR(b1_w.as_ptr()),
        },
        TASKDIALOG_BUTTON {
            nButtonID: ID_BTN2,
            pszButtonText: PCWSTR(b2_w.as_ptr()),
        },
    ];

    let config = TASKDIALOGCONFIG {
        cbSize: std::mem::size_of::<TASKDIALOGCONFIG>() as u32,
        hwndParent: HWND::default(),
        hInstance: Default::default(),
        dwFlags: TASKDIALOG_FLAGS(TDF_ALLOW_DIALOG_CANCELLATION.0),
        dwCommonButtons: Default::default(),
        pszWindowTitle: PCWSTR(title_w.as_ptr()),
        Anonymous1: TASKDIALOGCONFIG_0 {
            pszMainIcon: TD_WARNING_ICON,
        },
        pszMainInstruction: PCWSTR(instr_w.as_ptr()),
        pszContent: PCWSTR(body_w.as_ptr()),
        cButtons: buttons.len() as u32,
        pButtons: buttons.as_ptr(),
        nDefaultButton: ID_BTN1,
        cRadioButtons: 0,
        pRadioButtons: std::ptr::null(),
        nDefaultRadioButton: 0,
        pszVerificationText: PCWSTR::null(),
        pszExpandedInformation: PCWSTR::null(),
        pszExpandedControlText: PCWSTR::null(),
        pszCollapsedControlText: PCWSTR::null(),
        Anonymous2: TASKDIALOGCONFIG_1 {
            pszFooterIcon: PCWSTR::null(),
        },
        pszFooter: PCWSTR::null(),
        pfCallback: None,
        lpCallbackData: 0,
        cxWidth: 0,
    };

    let mut clicked: i32 = 0;
    unsafe {
        if TaskDialogIndirect(&config, Some(&mut clicked), None, None).is_err() {
            return DialogChoice::Primary; // on API failure, behave as if default was selected
        }
    }
    let _ = PWSTR::null(); // silence unused-import warnings if any
    match clicked {
        ID_BTN1 => DialogChoice::Primary,
        ID_BTN2 => DialogChoice::Secondary,
        _ => DialogChoice::Cancelled,
    }
}
