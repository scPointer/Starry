extern crate alloc;
use alloc::string::ToString;
use alloc::vec::Vec;
use axfs::api::{lookup, path_exists, FileIO, Kstat, OpenFlags};
use axlog::{debug, info};
use axprocess::link::FilePath;
use axsync::Mutex;

use crate::syscall::ErrorNo;

use super::{new_dir, new_fd, normal_file_mode, StMode};
// use crate::link::{real_path};

/// 挂载的文件系统。
/// 目前"挂载"的语义是，把一个文件当作文件系统读写
pub struct MountedFs {
    //pub inner: Arc<Mutex<FATFileSystem>>,
    pub device: FilePath,
    pub mnt_dir: FilePath,
}

impl MountedFs {
    pub fn new(device: &FilePath, mnt_dir: &FilePath) -> Self {
        assert!(
            device.is_file() && mnt_dir.is_dir(),
            "device must be a file and mnt_dir must be a dir"
        );
        Self {
            device: device.clone(),
            mnt_dir: mnt_dir.clone(),
        }
    }
    #[allow(unused)]
    pub fn device(&self) -> FilePath {
        self.device.clone()
    }

    pub fn mnt_dir(&self) -> FilePath {
        self.mnt_dir.clone()
    }
}

/// 已挂载的文件系统(设备)。
/// 注意启动时的文件系统不在这个 vec 里，它在 mod.rs 里。
static MOUNTED: Mutex<Vec<MountedFs>> = Mutex::new(Vec::new());

/// 挂载一个fatfs类型的设备
pub fn mount_fat_fs(device_path: &FilePath, mount_path: &FilePath) -> bool {
    // // device_path需要链接转换, mount_path不需要, 因为目前目录没有链接  // 暂时只有Open过的文件会加入到链接表，所以这里先不转换
    // debug!("mounting {} to {}", device_path.path(), mount_path.path());
    // if let Some(true_device_path) = real_path(device_path) {
    if path_exists(mount_path.path()) {
        MOUNTED.lock().push(MountedFs::new(device_path, mount_path));
        info!("mounted {} to {}", device_path.path(), mount_path.path());
        return true;
    }
    // }
    info!(
        "mount failed: {} to {}",
        device_path.path(),
        mount_path.path()
    );
    false
}

/// 卸载一个fatfs类型的设备
pub fn umount_fat_fs(mount_path: &FilePath) -> bool {
    let mut mounted = MOUNTED.lock();
    let mut i = 0;
    while i < mounted.len() {
        if mounted[i].mnt_dir().equal_to(mount_path) {
            mounted.remove(i);
            info!("umounted {}", mount_path.path());
            return true;
        }
        i += 1;
    }
    info!("umount failed: {}", mount_path.path());
    false
}

/// 检查一个路径是否已经被挂载
pub fn check_mounted(path: &FilePath) -> bool {
    let mounted = MOUNTED.lock();
    for m in mounted.iter() {
        if path.start_with(&m.mnt_dir()) {
            debug!("{} is mounted", path.path());
            return true;
        }
    }
    false
}

/// 根据给定的路径获取对应的文件stat
pub fn get_stat_in_fs(path: &FilePath) -> Result<Kstat, isize> {
    // 根目录算作一个简单的目录文件，不使用特殊的stat
    // 否则在fat32中查找
    let real_path = path.path();
    let mut ans = Kstat::default();
    if real_path.starts_with("/var")
        || real_path.starts_with("/dev")
        || real_path.starts_with("/tmp")
    {
        if path.is_dir() {
            ans.st_dev = 2;
            ans.st_mode = normal_file_mode(StMode::S_IFDIR).bits();
            return Ok(ans);
        }
        if let Ok(node) = lookup(path.path()) {
            let mut stat = Kstat::default();
            stat.st_nlink = 1;
            // 先检查是否在vfs中存在对应文件
            // 判断是在哪个vfs中
            if node
                .as_any()
                .downcast_ref::<axfs::axfs_devfs::DirNode>()
                .is_some()
                || node
                    .as_any()
                    .downcast_ref::<axfs::axfs_ramfs::DirNode>()
                    .is_some()
            {
                stat.st_dev = 2;
                stat.st_mode = normal_file_mode(StMode::S_IFDIR).bits();
                return Ok(stat);
            } else if node
                .as_any()
                .downcast_ref::<axfs::axfs_devfs::ZeroDev>()
                .is_some()
                || node
                    .as_any()
                    .downcast_ref::<axfs::axfs_devfs::NullDev>()
                    .is_some()
            {
                stat.st_mode = normal_file_mode(StMode::S_IFCHR).bits();
                return Ok(stat);
            } else if node
                .as_any()
                .downcast_ref::<axfs::axfs_ramfs::FileNode>()
                .is_some()
            {
                stat.st_mode = normal_file_mode(StMode::S_IFREG).bits();
                stat.st_size = node.get_attr().unwrap().size();
                return Ok(stat);
            }
        }
    }
    if !real_path.ends_with("/") && !real_path.ends_with("include") {
        // 是文件
        return if let Ok(file) = new_fd(real_path.to_string(), 0.into()) {
            match file.get_stat() {
                Ok(stat) => Ok(stat),
                Err(e) => {
                    debug!("get stat error: {:?}", e);
                    Err(ErrorNo::EINVAL as isize)
                }
            }
        } else {
            Err(ErrorNo::ENOENT as isize)
        };
    } else {
        // 是目录
        return if let Ok(dir) = new_dir(real_path.to_string(), OpenFlags::DIR) {
            match dir.get_stat() {
                Ok(stat) => Ok(stat),
                Err(e) => {
                    debug!("get stat error: {:?}", e);
                    Err(ErrorNo::EINVAL as isize)
                }
            }
        } else {
            Err(ErrorNo::ENOENT as isize)
        };
    }
}
