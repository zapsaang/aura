use std::fs::{File, OpenOptions};
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
#[cfg(unix)]
use std::os::unix::io::AsRawFd;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use memmap2::{MmapMut, MmapOptions};

use aura_common::{
    write_double_buffer, AuraError, AuraResult, TelemetryArchive, SHM_FILE_MODE, SHM_SIZE,
};

pub struct ShmHandle {
    mmap: MmapMut,
    _lock_file: File,
}

impl ShmHandle {
    pub fn new(path: &Path) -> AuraResult<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        // Acquire exclusive lock on a separate lockfile to prevent two daemons racing
        let lock_path = path.with_extension("lock");
        let lock_file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(&lock_path)?;

        #[cfg(unix)]
        {
            let fd = lock_file.as_raw_fd();
            let rc = unsafe { libc::flock(fd, libc::LOCK_EX | libc::LOCK_NB) };
            if rc != 0 {
                let err = std::io::Error::last_os_error();
                if err.raw_os_error() == Some(libc::EWOULDBLOCK) {
                    return Err(AuraError::AlreadyRunning);
                }
                return Err(err.into());
            }
        }

        if path.is_symlink() {
            return Err(AuraError::Security(format!(
                "refusing to open symlink for shared memory: {}",
                path.display()
            )));
        }

        if path.exists() {
            let file = OpenOptions::new()
                .read(true)
                .write(true)
                .custom_flags(libc::O_NOFOLLOW)
                .open(path)?;

            #[cfg(unix)]
            {
                let fd = file.as_raw_fd();
                let mut stat_buf = std::mem::MaybeUninit::<libc::stat>::uninit();
                if unsafe { libc::fstat(fd, stat_buf.as_mut_ptr()) } != 0 {
                    return Err(std::io::Error::last_os_error().into());
                }
                let stat = unsafe { stat_buf.assume_init() };

                if stat.st_uid != unsafe { libc::geteuid() } {
                    return Err(AuraError::Security(format!(
                        "SHM owned by uid {}, expected {}",
                        stat.st_uid,
                        unsafe { libc::geteuid() }
                    )));
                }

                let raw_mode = stat.st_mode & 0o777;
                if raw_mode != SHM_FILE_MODE as _ {
                    return Err(AuraError::Security(format!(
                        "SHM has mode {:o}, expected {:o}",
                        raw_mode, SHM_FILE_MODE
                    )));
                }

                if stat.st_size != SHM_SIZE as i64 {
                    return Err(AuraError::Security(format!(
                        "SHM size {} != expected {}",
                        stat.st_size, SHM_SIZE
                    )));
                }

                if (stat.st_mode & libc::S_IFMT) != libc::S_IFREG {
                    return Err(AuraError::Security("SHM is not a regular file".into()));
                }

                let rc = unsafe { libc::fchmod(fd, SHM_FILE_MODE as libc::mode_t) };
                if rc != 0 {
                    return Err(std::io::Error::last_os_error().into());
                }

                let rc = unsafe { libc::ftruncate(fd, SHM_SIZE as libc::off_t) };
                if rc != 0 {
                    return Err(std::io::Error::last_os_error().into());
                }
            }

            #[cfg(not(unix))]
            file.set_len(SHM_SIZE as u64)?;

            let mmap = unsafe { MmapOptions::new().len(SHM_SIZE).map_mut(&file)? };
            return Ok(Self {
                mmap,
                _lock_file: lock_file,
            });
        }

        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default();
        let tmp_path = path.with_extension(format!("tmp.{}.{}", std::process::id(), nonce));

        if tmp_path.exists() {
            std::fs::remove_file(&tmp_path)?;
        }

        if let Some(parent) = path.parent() {
            let tmp_prefix = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("aura_state");
            if let Ok(entries) = std::fs::read_dir(parent) {
                for entry in entries.flatten() {
                    if let Some(name) = entry.file_name().to_str() {
                        if name.starts_with(&format!("{}.tmp.", tmp_prefix)) {
                            let _ = std::fs::remove_file(entry.path());
                        }
                    }
                }
            }
        }

        let mut opts = OpenOptions::new();
        opts.read(true).write(true).create_new(true);
        #[cfg(unix)]
        {
            opts.mode(SHM_FILE_MODE);
            opts.custom_flags(libc::O_NOFOLLOW);
        }

        let file = opts.open(&tmp_path)?;

        #[cfg(unix)]
        {
            let fd = file.as_raw_fd();
            let rc = unsafe { libc::fchmod(fd, SHM_FILE_MODE as libc::mode_t) };
            if rc != 0 {
                return Err(std::io::Error::last_os_error().into());
            }

            let rc = unsafe { libc::ftruncate(fd, SHM_SIZE as libc::off_t) };
            if rc != 0 {
                return Err(std::io::Error::last_os_error().into());
            }
        }

        #[cfg(not(unix))]
        file.set_len(SHM_SIZE as u64)?;

        std::fs::rename(&tmp_path, path)?;

        let mmap = unsafe { MmapOptions::new().len(SHM_SIZE).map_mut(&file)? };

        Ok(Self {
            mmap,
            _lock_file: lock_file,
        })
    }

    pub fn write(&mut self, telemetry: &mut TelemetryArchive) -> AuraResult<()> {
        telemetry.checksum = 0;
        telemetry.checksum = telemetry.calculate_checksum();
        unsafe {
            write_double_buffer(self.mmap.as_mut_ptr(), telemetry);
        }
        Ok(())
    }
}
