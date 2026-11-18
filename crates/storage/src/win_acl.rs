//! Owner-only access control for directories on Windows.
#![cfg(windows)]
#![allow(unsafe_code)]

use std::io;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, LocalFree};
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
    ConvertStringSidToSidW, GetNamedSecurityInfoW, SDDL_REVISION_1, SE_FILE_OBJECT,
    SetNamedSecurityInfoW,
};
use windows_sys::Win32::Security::{
    ACCESS_ALLOWED_ACE, ACL, DACL_SECURITY_INFORMATION, EqualSid, GetAce,
    GetSecurityDescriptorControl, GetSecurityDescriptorDacl, GetTokenInformation,
    PROTECTED_DACL_SECURITY_INFORMATION, SE_DACL_PROTECTED, TOKEN_QUERY, TOKEN_USER, TokenUser,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

fn wide(s: &std::ffi::OsStr) -> Vec<u16> {
    s.encode_wide().chain(std::iter::once(0)).collect()
}

fn last_err() -> io::Error {
    io::Error::last_os_error()
}

/// The current user's SID as a string such as `S-1-5-21-...`.
pub(crate) fn current_user_sid_string() -> io::Result<String> {
    unsafe {
        let mut token: HANDLE = 0;
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return Err(last_err());
        }
        let result = (|| {
            let mut len = 0u32;
            GetTokenInformation(token, TokenUser, std::ptr::null_mut(), 0, &mut len);
            if len == 0 {
                return Err(last_err());
            }
            // u64 backing keeps the buffer aligned for TOKEN_USER.
            let mut buf = vec![0u64; (len as usize).div_ceil(8)];
            if GetTokenInformation(token, TokenUser, buf.as_mut_ptr().cast(), len, &mut len) == 0 {
                return Err(last_err());
            }
            let user = &*(buf.as_ptr().cast::<TOKEN_USER>());
            let mut sid_str: *mut u16 = std::ptr::null_mut();
            if ConvertSidToStringSidW(user.User.Sid, &mut sid_str) == 0 {
                return Err(last_err());
            }
            let mut n = 0;
            while *sid_str.add(n) != 0 {
                n += 1;
            }
            let s = String::from_utf16_lossy(std::slice::from_raw_parts(sid_str, n));
            LocalFree(sid_str.cast());
            Ok(s)
        })();
        CloseHandle(token);
        result
    }
}

/// True when the DACL of `path` is already protected and holds exactly one allow ACE for the
/// current user that is inherited by files and directories (what `restrict_to_owner` writes).
pub(crate) fn is_owner_only(path: &Path) -> io::Result<bool> {
    let me = wide(std::ffi::OsStr::new(&current_user_sid_string()?));
    let target = wide(path.as_os_str());
    unsafe {
        let mut my_sid = std::ptr::null_mut();
        if ConvertStringSidToSidW(me.as_ptr(), &mut my_sid) == 0 {
            return Err(last_err());
        }
        let mut dacl: *mut ACL = std::ptr::null_mut();
        let mut sd = std::ptr::null_mut();
        let rc = GetNamedSecurityInfoW(
            target.as_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut dacl,
            std::ptr::null_mut(),
            &mut sd,
        );
        if rc != 0 {
            LocalFree(my_sid.cast());
            return Err(io::Error::from_raw_os_error(rc as i32));
        }
        let result = (|| {
            let mut control = 0u16;
            let mut rev = 0u32;
            if GetSecurityDescriptorControl(sd, &mut control, &mut rev) == 0 {
                return Err(last_err());
            }
            if control & SE_DACL_PROTECTED == 0 || dacl.is_null() || (*dacl).AceCount != 1 {
                return Ok(false);
            }
            let mut ace = std::ptr::null_mut();
            if GetAce(dacl, 0, &mut ace) == 0 {
                return Err(last_err());
            }
            let ace = &mut *(ace.cast::<ACCESS_ALLOWED_ACE>());
            // ACCESS_ALLOWED_ACE_TYPE = 0; OBJECT_INHERIT_ACE | CONTAINER_INHERIT_ACE = 3.
            let sid = std::ptr::addr_of_mut!(ace.SidStart).cast();
            Ok(ace.Header.AceType == 0
                && ace.Header.AceFlags & 3 == 3
                && EqualSid(sid, my_sid) != 0)
        })();
        LocalFree(sd.cast());
        LocalFree(my_sid.cast());
        result
    }
}

/// Replace the DACL of `path` with a protected one that grants full control to
/// the current user only, inherited by new files (OI) and directories (CI).
pub(crate) fn restrict_to_owner(path: &Path) -> io::Result<()> {
    let sid = current_user_sid_string()?;
    let sddl = wide(std::ffi::OsStr::new(&format!("D:P(A;OICI;FA;;;{sid})")));
    let target = wide(path.as_os_str());
    unsafe {
        let mut sd = std::ptr::null_mut();
        if ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            SDDL_REVISION_1,
            &mut sd,
            std::ptr::null_mut(),
        ) == 0
        {
            return Err(last_err());
        }
        let result = (|| {
            let (mut present, mut defaulted) = (0i32, 0i32);
            let mut dacl: *mut ACL = std::ptr::null_mut();
            if GetSecurityDescriptorDacl(sd, &mut present, &mut dacl, &mut defaulted) == 0
                || present == 0
                || dacl.is_null()
            {
                return Err(last_err());
            }
            let rc = SetNamedSecurityInfoW(
                target.as_ptr(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                dacl,
                std::ptr::null(),
            );
            if rc != 0 {
                return Err(io::Error::from_raw_os_error(rc as i32));
            }
            Ok(())
        })();
        LocalFree(sd.cast());
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Read the DACL back: (protected, allow-ACE count, all ACEs allow the current user).
    fn read_dacl(path: &Path) -> (bool, usize, bool) {
        let me = wide(std::ffi::OsStr::new(&current_user_sid_string().unwrap()));
        let target = wide(path.as_os_str());
        unsafe {
            let mut my_sid = std::ptr::null_mut();
            assert_ne!(ConvertStringSidToSidW(me.as_ptr(), &mut my_sid), 0);
            let mut dacl: *mut ACL = std::ptr::null_mut();
            let mut sd = std::ptr::null_mut();
            let rc = GetNamedSecurityInfoW(
                target.as_ptr(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut dacl,
                std::ptr::null_mut(),
                &mut sd,
            );
            assert_eq!(rc, 0);
            let mut control = 0u16;
            let mut rev = 0u32;
            assert_ne!(GetSecurityDescriptorControl(sd, &mut control, &mut rev), 0);
            let count = (*dacl).AceCount as usize;
            let mut all_me = true;
            for i in 0..count {
                let mut ace = std::ptr::null_mut();
                assert_ne!(GetAce(dacl, i as u32, &mut ace), 0);
                let ace = &mut *(ace.cast::<ACCESS_ALLOWED_ACE>());
                let is_allow = ace.Header.AceType == 0;
                let sid = std::ptr::addr_of_mut!(ace.SidStart).cast();
                all_me &= is_allow && EqualSid(sid, my_sid) != 0;
            }
            LocalFree(sd.cast());
            LocalFree(my_sid.cast());
            (control & SE_DACL_PROTECTED != 0, count, all_me)
        }
    }

    #[test]
    fn private_dir_has_single_owner_ace_inherited_by_files() {
        let t = tempfile::tempdir().unwrap();
        let dir = t.path().join("data");
        crate::data_dir::ensure_private_dir(&dir).unwrap();
        assert_eq!(read_dacl(&dir), (true, 1, true));
        let file = dir.join("f.json");
        std::fs::write(&file, "x").unwrap();
        let (_, count, all_me) = read_dacl(&file);
        assert_eq!((count, all_me), (1, true));
        // An existing, wider dir is tightened.
        let wide_dir = t.path().join("existing");
        std::fs::create_dir(&wide_dir).unwrap();
        crate::data_dir::ensure_private_dir(&wide_dir).unwrap();
        assert_eq!(read_dacl(&wide_dir), (true, 1, true));
        assert!(is_owner_only(&wide_dir).unwrap());
        assert!(!is_owner_only(t.path()).unwrap());
    }
}
