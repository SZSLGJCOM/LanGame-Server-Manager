//! Read-only shutdown evidence for the one verified native Moria build.
use super::*;
use std::io::Read;
use std::os::windows::fs::OpenOptionsExt;
use std::time::Instant;
use windows_sys::Win32::Security::Cryptography::{
    BCryptCloseAlgorithmProvider, BCryptCreateHash, BCryptDestroyHash, BCryptFinishHash,
    BCryptHashData, BCryptOpenAlgorithmProvider,
};

const IMAGE_BYTES: u64 = 133_941_248;
const IMAGE_TIMESTAMP: u32 = 0x698b_9222;
const IMAGE_SIZE: u32 = 0x082c_1000;
const IMAGE_SHA256: [u8; 32] = [
    0x45, 0x13, 0xfb, 0x58, 0xca, 0x31, 0xa3, 0xc7, 0xc7, 0x64, 0x0e, 0xbe, 0xff, 0x97, 0x17, 0xef,
    0x68, 0x77, 0x6b, 0x92, 0x29, 0x86, 0x03, 0x6f, 0x0c, 0x5b, 0x32, 0x5d, 0xd2, 0x82, 0x55, 0xfb,
];
const EXIT_REQUEST: usize = 0x0798_91ad;
const CRITICAL_ERROR: usize = 0x0798_91a6;
const WINDOWS: [(usize, &[u8]); 3] = [
    (
        0x0297_069e,
        &[
            0xc6, 0x05, 0x08, 0x8b, 0x01, 0x05, 0x01, 0x48, 0x83, 0xc4, 0x40, 0x5b, 0xc3,
        ],
    ),
    (
        0x022f_53f8,
        &[
            0x4c, 0x8d, 0x4c, 0x24, 0x34, 0x4c, 0x89, 0x74, 0x24, 0x20, 0x41, 0xb8, 0x00, 0x04,
            0x00, 0x00, 0x48, 0x8d, 0x55, 0x40, 0x48, 0x8b, 0xce, 0xff, 0x15, 0x93, 0xe3, 0x26,
            0x03, 0x85, 0xc0, 0x0f, 0x84, 0x02, 0x02, 0x00, 0x00,
        ],
    ),
    (
        0x022d_26ac,
        &[
            0x80, 0x7b, 0x10, 0x04, 0x74, 0x04, 0xc6, 0x43, 0x10, 0x03, 0x40, 0x38, 0xab, 0x00,
            0x01, 0x00, 0x00, 0x74, 0x0d, 0xff, 0x15, 0xd3, 0x10, 0x29, 0x03, 0x40, 0x88, 0xab,
            0x00, 0x01, 0x00, 0x00, 0x48, 0x8b, 0x43, 0x28, 0x48, 0x8d, 0x4b, 0x28, 0xff, 0x50,
            0x10,
        ],
    ),
];

pub(super) struct Profile {
    pub(super) process: WindowsProcessHandle,
    base: usize,
    // Keep the hashed image locked against replacement until this exchange ends.
    _image: File,
}

impl Profile {
    pub(super) fn open(
        pid: u32,
        identity: &ProcessIdentity,
        deadline: Instant,
    ) -> Result<Self, RuntimeProcessError> {
        let process = open_verified_windows_process(pid, identity, 0x10)?
            .ok_or_else(|| error(pid, "Moria Shipping exited before shutdown authorization"))?;
        let image = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(&identity.image_path)
            .map_err(|_| {
                error(
                    pid,
                    "Moria shutdown image could not be pinned for verification",
                )
            })?;
        if image
            .metadata()
            .map_err(|_| error(pid, "Moria shutdown image metadata unavailable"))?
            .len()
            != IMAGE_BYTES
            || digest(&image, deadline).map_err(|_| {
                error(
                    pid,
                    "Moria shutdown image hash unavailable or deadline expired",
                )
            })? != IMAGE_SHA256
        {
            return Err(error(
                pid,
                "Moria shutdown read cancellation requires verified native build 21872765",
            ));
        }
        let base = module_base(pid, identity)?;
        let profile = Self {
            process,
            base,
            _image: image,
        };
        let pe = u32::from_le_bytes(profile.read::<4>(0x3c)?);
        if pe > 4096
            || profile.read::<4>(pe as usize)? != *b"PE\0\0"
            || u32::from_le_bytes(profile.read::<4>(pe as usize + 8)?) != IMAGE_TIMESTAMP
            || u32::from_le_bytes(profile.read::<4>(pe as usize + 80)?) != IMAGE_SIZE
        {
            return Err(error(pid, "Moria loaded shutdown image header changed"));
        }
        for (rva, expected) in WINDOWS {
            let mut actual = [0u8; 64];
            profile.read_bytes(rva, &mut actual[..expected.len()])?;
            if actual[..expected.len()] != *expected {
                return Err(error(pid, "Moria loaded shutdown code changed"));
            }
        }
        if profile.exit_requested()? {
            return Err(error(
                pid,
                "Moria was already exiting before the fixed shutdown command",
            ));
        }
        Ok(profile)
    }

    pub(super) fn exit_requested(&self) -> Result<bool, RuntimeProcessError> {
        native_exit_state(
            self.read::<1>(EXIT_REQUEST)?[0],
            self.read::<1>(CRITICAL_ERROR)?[0],
        )
        .ok_or_else(|| {
            error(
                self.process.pid,
                "Moria native shutdown state is invalid or critical",
            )
        })
    }

    fn read<const N: usize>(&self, rva: usize) -> Result<[u8; N], RuntimeProcessError> {
        let mut bytes = [0; N];
        self.read_bytes(rva, &mut bytes)?;
        Ok(bytes)
    }

    fn read_bytes(&self, rva: usize, bytes: &mut [u8]) -> Result<(), RuntimeProcessError> {
        let address = self
            .base
            .checked_add(rva)
            .ok_or_else(|| error(self.process.pid, "Moria shutdown address overflow"))?;
        let mut count = 0;
        if unsafe {
            ReadProcessMemory(
                self.process.raw,
                address as *const c_void,
                bytes.as_mut_ptr().cast(),
                bytes.len(),
                &mut count,
            )
        } == 0
            || count != bytes.len()
        {
            return Err(error(
                self.process.pid,
                "Moria shutdown state could not be read",
            ));
        }
        Ok(())
    }
}

fn native_exit_state(request: u8, critical: u8) -> Option<bool> {
    (request <= 1 && critical == 0).then_some(request == 1)
}

struct Algorithm(*mut c_void);
impl Drop for Algorithm {
    fn drop(&mut self) {
        unsafe {
            BCryptCloseAlgorithmProvider(self.0, 0);
        }
    }
}
struct Hash(*mut c_void);
impl Drop for Hash {
    fn drop(&mut self) {
        unsafe {
            BCryptDestroyHash(self.0);
        }
    }
}

fn digest(mut image: &File, deadline: Instant) -> std::io::Result<[u8; 32]> {
    let mut algorithm = Algorithm(std::ptr::null_mut());
    let name: Vec<u16> = "SHA256".encode_utf16().chain([0]).collect();
    if unsafe { BCryptOpenAlgorithmProvider(&mut algorithm.0, name.as_ptr(), std::ptr::null(), 0) }
        < 0
    {
        return Err(std::io::Error::other("SHA256 provider unavailable"));
    }
    let mut hash = Hash(std::ptr::null_mut());
    if unsafe {
        BCryptCreateHash(
            algorithm.0,
            &mut hash.0,
            std::ptr::null_mut(),
            0,
            std::ptr::null(),
            0,
            0,
        )
    } < 0
    {
        return Err(std::io::Error::other("SHA256 state unavailable"));
    }
    let mut bytes = [0u8; 64 * 1024];
    loop {
        if Instant::now() >= deadline {
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "shutdown hash deadline",
            ));
        }
        let length = image.read(&mut bytes)?;
        if length == 0 {
            break;
        }
        if unsafe { BCryptHashData(hash.0, bytes.as_ptr(), length as u32, 0) } < 0 {
            return Err(std::io::Error::other("SHA256 update failed"));
        }
    }
    let mut result = [0u8; 32];
    if unsafe { BCryptFinishHash(hash.0, result.as_mut_ptr(), result.len() as u32, 0) } < 0 {
        return Err(std::io::Error::other("SHA256 completion failed"));
    }
    Ok(result)
}

#[repr(C)]
struct ModuleEntry {
    size: u32,
    id: u32,
    pid: u32,
    global_usage: u32,
    process_usage: u32,
    base: *mut u8,
    image_size: u32,
    module: *mut c_void,
    name: [u16; 256],
    path: [u16; 260],
}

fn module_base(pid: u32, identity: &ProcessIdentity) -> Result<usize, RuntimeProcessError> {
    let snapshot = Handle(unsafe { CreateToolhelp32Snapshot(0x08, pid) });
    if snapshot.0.is_null() || snapshot.0 as isize == -1 {
        return Err(error(pid, "Moria shutdown module snapshot unavailable"));
    }
    let mut entry: ModuleEntry = unsafe { std::mem::zeroed() };
    entry.size = std::mem::size_of::<ModuleEntry>() as u32;
    if unsafe { Module32FirstW(snapshot.0, &mut entry) } == 0
        || entry.pid != pid
        || entry.base.is_null()
        || entry.image_size != IMAGE_SIZE
    {
        return Err(error(pid, "Moria shutdown primary module unavailable"));
    }
    let end = entry
        .path
        .iter()
        .position(|value| *value == 0)
        .ok_or_else(|| error(pid, "Moria shutdown module path is incomplete"))?;
    let path = String::from_utf16(&entry.path[..end])
        .map_err(|_| error(pid, "Moria shutdown module path is invalid"))?;
    if normalized_process_image_path(&path) != normalized_process_image_path(&identity.image_path) {
        return Err(error(pid, "Moria shutdown primary module identity changed"));
    }
    Ok(entry.base as usize)
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn ReadProcessMemory(
        process: *mut c_void,
        address: *const c_void,
        buffer: *mut c_void,
        length: usize,
        count: *mut usize,
    ) -> i32;
    fn Module32FirstW(snapshot: *mut c_void, entry: *mut ModuleEntry) -> i32;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_noncritical_native_exit_request_allows_console_read_cancellation() {
        assert_eq!(native_exit_state(0, 0), Some(false));
        assert_eq!(native_exit_state(1, 0), Some(true));
        assert_eq!(native_exit_state(2, 0), None);
        assert_eq!(native_exit_state(1, 1), None);
        assert_eq!(native_exit_state(1, 255), None);
    }

    #[test]
    fn engine_exit_instruction_references_the_matching_pdb_flag() {
        let (rva, bytes) = WINDOWS[0];
        let offset = i32::from_le_bytes(bytes[2..6].try_into().unwrap());
        assert_eq!((rva as i64 + 7 + offset as i64) as usize, EXIT_REQUEST);
        assert_eq!(&bytes[..2], &[0xc6, 0x05]);
        assert_eq!(bytes[6], 1);
    }

    #[test]
    fn shutdown_hash_uses_sha256_and_honors_an_expired_exchange_deadline() {
        let root = crate::test_support::unique_test_root();
        std::fs::create_dir(&root).unwrap();
        let path = root.join("sha256-vector");
        std::fs::write(&path, b"abc").unwrap();
        let image = File::open(&path).unwrap();
        assert_eq!(
            digest(&image, Instant::now() + std::time::Duration::from_secs(5)).unwrap(),
            [
                0xba, 0x78, 0x16, 0xbf, 0x8f, 0x01, 0xcf, 0xea, 0x41, 0x41, 0x40, 0xde, 0x5d, 0xae,
                0x22, 0x23, 0xb0, 0x03, 0x61, 0xa3, 0x96, 0x17, 0x7a, 0x9c, 0xb4, 0x10, 0xff, 0x61,
                0xf2, 0x00, 0x15, 0xad
            ]
        );
        assert_eq!(
            digest(&image, Instant::now()).unwrap_err().kind(),
            std::io::ErrorKind::TimedOut
        );
        drop(image);
        std::fs::remove_dir_all(root).unwrap();
    }
}
