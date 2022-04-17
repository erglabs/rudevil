extern crate notify;
pub use config::Config;
use file_owner::{set_group, set_owner};
pub use guard::*;
pub use notify::{watcher, DebouncedEvent, RecursiveMode, Watcher};
pub use std::os::unix::prelude::PermissionsExt;
pub use std::path::PathBuf;
pub use std::sync::mpsc::channel;
pub use std::time::Duration;
use tokio::fs::remove_dir;
use tracing::instrument;
pub use tracing_subscriber::prelude::__tracing_subscriber_SubscriberExt;
pub use tracing_subscriber::util::SubscriberInitExt;
pub use unwrap_return::*;
pub use users::{Groups, Users, UsersCache};

use regex::Regex;

use std::process::exit;
use sys_mount::{self, FilesystemType};
use sys_mount::{unmount, UnmountFlags};
use sys_mount::{Mount, MountFlags, SupportedFilesystems};

// we die if we can't find the group, there is no point in running
#[instrument]
async fn find_gid(name: String) -> u32 {
    let cache = UsersCache::new();
    let group = cache.get_group_by_name(&name);
    guard::guard!(let group = group.unwrap() else { exit(23) } );
    tracing::info!(
        "found group: {} with id {}",
        group.name().to_string_lossy(),
        group.gid()
    );
    group.gid()
}

#[instrument]
async fn find_uid(name: String) -> u32 {
    tracing::info!("requesting user with {}", name);
    let cache = UsersCache::new();
    let user = cache.get_user_by_name(&name);
    guard::guard!(let Some(user) = user else { exit(24) } );
    tracing::info!(
        "found user: {} with id {}",
        user.name().to_string_lossy(),
        user.uid()
    );
    user.uid()
}

#[instrument]
async fn find_workir(path: String) -> PathBuf {
    let res = PathBuf::from(&path);
    if res.is_symlink() {
        tracing::error!("path does not exists, do not waste my time, got {}", &path);
        exit(-27);
    };
    if !res.is_dir() {
        tracing::error!("desired path is invalid, not a dir");
        exit(-29);
    };
    if !res.starts_with("/") {
        tracing::error!("wordkir path must start at root, got {}", &path);
        exit(-25);
    };
    if !res.is_absolute() {
        tracing::error!("desired path must be absolute, got {}", &path);
        exit(-26);
    };
    if res.is_symlink() {
        tracing::error!("path can not be a simlink");
        exit(-30);
    };
    res
}

#[instrument]
async fn extract_device(path: &std::path::Path) -> Option<String> {
    guard!(let Some(p) = path.as_os_str().to_str() else {return None});
    guard!(let Some(res) = p.split_terminator('/').last() else {return None});
    Some(res.to_owned())
}

#[instrument]
async fn process_created(path: PathBuf, uid: u32, gid: u32, workingdir: PathBuf) {
    tracing::info!("processing {:?}", path);
    let re = Regex::new("/dev/sd[a-zA-Z]{1,2}[0-9]{1,2}").unwrap();
    if re.find(path.as_os_str().to_str().unwrap_or("")).is_none() {
        return;
    };

    guard!(let Some(device) = extract_device(path.as_path()).await else {
      tracing::warn!("couldn't match the device for some reason?");
      return;
    });

    let dest: std::path::PathBuf = workingdir.join(PathBuf::from(device));
    tracing::debug!("destpath is {:?}", dest);

    // since this point we need to remember to cleanup
    std::fs::create_dir(dest.clone()).unwrap();
    let metadata = std::fs::metadata(dest.clone()).unwrap();
    let mut permissions = metadata.permissions();
    permissions.set_mode(0o775);

    // set ownership before mounting
    if let Err(e) = set_owner(&path, uid) {
        tracing::error!("failed to set user ownership! {}", e);
        if let Err(e) = remove_dir(&path).await {
            tracing::error!("tried to cleanup but this happened: {:?}", e);
        }
    }
    if let Err(e) = set_group(&path, gid) {
        tracing::error!("failed to set group ownership! {}", e);
        if let Err(e) = remove_dir(&path).await {
            tracing::error!("tried to cleanup but this happened: {:?}", e);
        }
    }

    // Fetch a listed of supported file systems on this system. This will be used
    // as the fstype to `Mount::new`, as the `Auto` mount parameter.
    let supported = match SupportedFilesystems::new() {
        Ok(supported) => supported,
        Err(why) => {
            tracing::error!("failed to get supported file systems: {}", why);
            exit(1);
        }
    };
    // The source block will be mounted to the target directory, and the fstype is likely
    // one of the supported file systems.
    let result = Mount::builder()
        .fstype(FilesystemType::from(&supported))
        .flags(MountFlags::NODEV)
        .flags(MountFlags::NOSUID)
        .mount(path, dest);

    if let Err(e) = result {
        tracing::error!("mount failed, {}", e)
    }
}

#[instrument]
async fn process_removed(path: PathBuf, workingdir: PathBuf) {
    let re = Regex::new("/dev/sd[a-zA-Z]{1,2}[0-9]{1,2}").unwrap();
    if re.find(path.as_os_str().to_str().unwrap_or("")).is_none() {
        return;
    };

    guard!(let Some(device) = extract_device(path.as_path()).await else {
      tracing::warn!("couldn't match the device for some reason?");
      return;
    });
    tracing::info!("processing {:?}", path);

    let dest: std::path::PathBuf = workingdir.join(PathBuf::from(device));
    tracing::debug!("destpath is {:?}", dest);

    if let Err(why) = unmount(&dest, UnmountFlags::FORCE) {
        tracing::error!("failed to unmount {:?}: {}", path, why);
    }

    drop(remove_dir(dest).await);
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // ### logging setup
    tracing::info!("\n\nset DEBUGMODE to anything for easier debugging\n\n");
    let is_debug_mode = std::env::var("DEBUGMODE").map_or(false, |x| !x.is_empty());

    let subscriber = tracing_subscriber::registry().with(tracing_subscriber::EnvFilter::new(
        std::env::var("RUST_LOG").unwrap_or_else(|_| "debug".into()),
    ));
    if is_debug_mode {
        subscriber
            .with(
                tracing_subscriber::fmt::layer()
                    .with_line_number(true)
                    .with_thread_ids(true)
                    .with_thread_names(true)
                    .pretty(),
            )
            .init()
    } else {
        subscriber
            .with(tracing_subscriber::fmt::layer().compact())
            .init();
    }

    // ### preparations
    let settings = Config::builder()
        .add_source(config::Environment::with_prefix("RUDEVIL").separator("_"))
        .build()
        .unwrap();

    tracing::info!("config is {:?}", &settings);

    // // let out = settings.get_array("user")?;
    dbg!(&settings);
    // let r : String = settings.get("user")?;
    // dbg!(r);

    // lets default here
    let wanteduser: String = settings.get("user").unwrap_or_else(|_| "root".to_owned());
    let wantedgroup: String = settings
        .get("group")
        .unwrap_or_else(|_| "plugdev".to_owned());
    let wantedworkidr: String = settings
        .get("wordir")
        .unwrap_or_else(|_| "/storage".to_owned());

    // lets check the creds
    let user_id = find_uid(wanteduser.to_owned()).await;
    let group_id = find_gid(wantedgroup.to_owned()).await;
    let workdir = find_workir(wantedworkidr.to_owned()).await;

    tracing::trace!("The '{}' group has the ID {}", wantedgroup, group_id);
    tracing::trace!("The '{}' user has the ID {}", wanteduser, user_id);
    tracing::trace!("mounting filesystems at {:?}", &workdir);

    // setup watcher
    let (tx, rx) = channel();
    let mut watcher = watcher(tx, Duration::from_secs(1)).unwrap();
    watcher.watch("/dev", RecursiveMode::NonRecursive).unwrap();

    // setup router
    loop {
        match rx.recv() {
            Ok(DebouncedEvent::Create(path)) => {
                process_created(path, user_id, group_id, workdir.clone()).await;
            }
            Ok(DebouncedEvent::Remove(path)) => {
                process_removed(path, workdir.clone()).await;
            }
            Err(e) => tracing::error!("watch error: {:?}", e),
            _ => {} // do nothing on unsupported events,
        }
    }
}
