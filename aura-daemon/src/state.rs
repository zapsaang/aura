use std::fs::OpenOptions;
use std::io::ErrorKind;
#[cfg(unix)]
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::Path;
use std::sync::atomic::{fence, AtomicUsize, Ordering};

use memmap2::{MmapMut, MmapOptions};

use aura_common::{AuraError, AuraResult, TelemetryArchive, DATA_OFFSET, SHM_FILE_MODE, SHM_SIZE};

pub struct ShmHandle {
    mmap: MmapMut,
}

impl ShmHandle {
    pub fn new(path: &Path) -> AuraResult<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        if path.is_symlink() {
            return Err(AuraError::Security(format!(
                "refusing to open symlink for shared memory: {}",
                path.display()
            )));
        }

        let mut opts = OpenOptions::new();
        opts.read(true).write(true).create_new(true);
        #[cfg(unix)]
        {
            opts.mode(SHM_FILE_MODE);
            opts.custom_flags(libc::O_NOFOLLOW);
        }

        let file = match opts.open(path) {
            Ok(file) => {
                // create_new succeeded — set permissions to exact value (umask-independent)
                // This has a tiny TOCTOU window but O_NOFOLLOW on the open above
                // prevents symlink exploitation at open time.
                #[cfg(unix)]
                {
                    std::fs::set_permissions(path, std::fs::Permissions::from_mode(SHM_FILE_MODE))?;
                }
                file
            }
            Err(err) if err.kind() == ErrorKind::AlreadyExists => {
                validate_existing_shm(path)?;

                let mut opts2 = OpenOptions::new();
                opts2.read(true).write(true);
                #[cfg(unix)]
                {
                    opts2.custom_flags(libc::O_NOFOLLOW);
                }
                let file = opts2.open(path)?;
                // Repair permissions if they were wrong (daemon restarting with existing SHM)
                #[cfg(unix)]
                {
                    std::fs::set_permissions(path, std::fs::Permissions::from_mode(SHM_FILE_MODE))?;
                }
                file
            }
            Err(err) => return Err(err.into()),
        };

        file.set_len(SHM_SIZE as u64)?;

        let mmap = unsafe { MmapOptions::new().len(SHM_SIZE).map_mut(&file)? };

        Ok(Self { mmap })
    }

    pub fn write(&mut self, telemetry: &TelemetryArchive) -> AuraResult<()> {
        let base = self.mmap.as_mut_ptr();
        let version_ptr = base as *mut AtomicUsize;
        unsafe { (*version_ptr).fetch_add(1, Ordering::SeqCst) };
        fence(Ordering::Release);

        let dst = unsafe { base.add(DATA_OFFSET) };
        let src = telemetry as *const TelemetryArchive as *const u8;
        let len = std::mem::size_of::<TelemetryArchive>();
        unsafe {
            std::ptr::copy_nonoverlapping(src, dst, len);
        }

        fence(Ordering::Release);
        unsafe { (*version_ptr).fetch_add(1, Ordering::SeqCst) };
        Ok(())
    }
}

fn validate_existing_shm(path: &Path) -> AuraResult<()> {
    if path.is_symlink() {
        return Err(AuraError::Security(format!(
            "refusing to open symlink for shared memory: {}",
            path.display()
        )));
    }

    #[cfg(unix)]
    {
        let metadata = std::fs::metadata(path)?;
        let owner_uid = metadata.uid();
        let current_uid = unsafe { libc::geteuid() };
        if owner_uid != current_uid {
            return Err(AuraError::Security(format!(
                "shared memory file owner mismatch for {}: uid {} != {}",
                path.display(),
                owner_uid,
                current_uid
            )));
        }
        // Note: permission check omitted here — fallback path repairs permissions after open
    }

    Ok(())
}
