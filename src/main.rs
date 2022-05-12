use config::Config;
use file_owner::{set_group, set_owner};
use guard::*;
use notify::{watcher, DebouncedEvent, RecursiveMode, Watcher};
use regex::Regex;
use rudevil::notifications::desktop::notify;
use std::os::unix::prelude::PermissionsExt;
use std::path::PathBuf;
use std::sync::mpsc::channel;
use std::time::Duration;
use sys_mount::{self, FilesystemType};
use sys_mount::{unmount, UnmountFlags};
use sys_mount::{Mount, MountFlags, SupportedFilesystems};
use tokio::fs::remove_dir;
use tracing::instrument;
use tracing_subscriber::prelude::__tracing_subscriber_SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use users::{Groups, Users, UsersCache};

/// Translates desired group (in string) to group id
///
/// Returns anyhow::Result<u32>
///
/// Group is provided by config, we need to change it to the uid to be compatible
/// with all of the libraries that uses that to change i.e. permissions or
/// ownership.
///
/// # Errors
///
/// Returns Error if the group does not exists
/// Returns u32 if the group does exists
#[instrument]
async fn find_gid(name: String) -> anyhow::Result<u32> {
    let cache = UsersCache::new();
    let group = cache.get_group_by_name(&name);
    guard::guard!(let group = group.unwrap() else { anyhow::bail!("no such group") } );
    tracing::info!(
        "found group: {} with id {}",
        group.name().to_string_lossy(),
        group.gid()
    );
    Ok(group.gid())
}

/// Translates user (in string) to user id
///
/// Returns anyhow::Result<u32>
///
/// User is provided by config, we need to change it to the uid to be compatible
/// with all of the libraries that uses that to change i.e. permissions or
/// ownership.
///
/// # Errors
///
/// Returns Error if the user does not exists
/// Returns u32 if the user does exists
#[instrument]
async fn find_uid(name: String) -> anyhow::Result<u32> {
    tracing::info!("requesting user with {}", name);
    let cache = UsersCache::new();
    let user = cache.get_user_by_name(&name);
    guard::guard!(let Some(user) = user else { anyhow::bail!("no such group") } );
    tracing::info!(
        "found user: {} with id {}",
        user.name().to_string_lossy(),
        user.uid()
    );
    Ok(user.uid())
}

/// Validates whether the desired workdir exists
///
/// Returns anyhow::Result<PathBuf>
///
/// Checks if the desired workdir base is safe from security and
/// usage perspective. Errors out on any discrency.
/// - path must be absolute
/// - path must exist
/// - path must start with root "/"
/// - path must be a directory
/// - path can not be a symlink
///
/// # Errors
///
/// Returns Error if the group does not exists
/// Returns PathBuf if its okay
#[instrument]
async fn find_workir(path: String) -> anyhow::Result<PathBuf> {
    let res = PathBuf::from(&path);
    if res.is_symlink() {
        anyhow::bail!("path does not exists, do not waste my time, got {}", &path);
    };
    if !res.is_dir() {
        anyhow::bail!("desired path is invalid, not a dir");
    };
    if !res.starts_with("/") {
        anyhow::bail!("workdir path must start at root, got {}", &path);
    };
    if !res.is_absolute() {
        anyhow::bail!("desired path must be absolute, got {}", &path);
    };
    if res.is_symlink() {
        anyhow::bail!("path can not be a symlink");
    };
    Ok(res)
}

/// Validates the correlation between device and event
///
/// Returns Option<String>
/// Returns None in case there is something wrong.
///
/// Checks if the event and the path related to the event
/// is a valid and mountable device.
/// I was lazy with it so i am just matching the regexp.
/// On 99.4% systems this should be secure enough for now.
///
/// # Errors
///
/// none, hah :)
#[instrument]
async fn extract_device(path: &std::path::Path) -> Option<String> {
    Some(
        path.as_os_str()
            .to_str()?
            .split_terminator('/')
            .last()?
            .to_owned(),
    )
}

/// Handles the "created" event type - case when block device was added
///
/// Returns ()
///
/// Acts as a handler. Takes Path related to the event, runs it through extractors
/// and validators, and if everything checks out, creates the mounting directory
/// sets permissions for the user and mounts the drive. Internal state is prone to
/// getting poisoned so in some places it handles its own cleanup.
///
/// # Errors
///
/// nah, none, at least in theory ;)
/// However, if the filesystem is not supported, the path extractor fails or allowed
/// path do not match the drive, it will return early.
#[instrument]
pub async fn process_created(
    path: PathBuf,
    uid: u32,
    gid: u32,
    workingdir: PathBuf,
) -> anyhow::Result<()> {
    tracing::info!("processing {:?}", path);
    let re = Regex::new("/dev/sd[a-zA-Z]{1,2}[0-9]{1,2}").unwrap();
    if re.find(path.as_os_str().to_str().unwrap_or("")).is_none() {
        anyhow::bail!("wrong device name");
    };

    guard!(let Some(device) = extract_device(path.as_path()).await else {
      tracing::warn!("couldn't match the device for some reason?");
      anyhow::bail!("can't match");
    });

    tracing::info!("processing {:?}, event=created", path);
    notify(format!("mounting device: {}", device).as_str())?;

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
            anyhow::bail!("filesystem not supported");
        }
    };

    // The source block will be mounted to the target directory, and the fstype is likely
    // one of the supported file systems.
    let result = Mount::builder()
        .fstype(FilesystemType::from(&supported))
        .flags(MountFlags::NODEV)
        .flags(MountFlags::NOSUID)
        .mount(&path, dest);

    if let Err(e) = result {
        tracing::error!("mount failed, {}", e)
    }
    Ok(())
}

/// Handles the "removed" event type - case when block device was removed
///
/// Returns ()
///
/// Acts as a handler. Takes Path related to the event, runs it through extractors
/// and validators. Performas a cleanup of the device if it was removed to early.
/// It does not take into account events like manual unmount of the drive etc.
/// If the directory was not created nor mounted by rudevil, but it exists AND the
/// drive was removed, then it will also remove the directory.
///
/// # Errors
///
/// Should be none. Just early returns in case the path do not match.
#[instrument]
async fn process_removed(path: PathBuf, workingdir: PathBuf) -> anyhow::Result<()> {
    let re = Regex::new("/dev/sd[a-zA-Z]{1,2}[0-9]{1,2}").unwrap();
    if re.find(path.as_os_str().to_str().unwrap_or("")).is_none() {
        return Ok(());
    };

    guard!(let Some(device) = extract_device(path.as_path()).await else {
      tracing::warn!("couldn't match the device for some reason?");
      return Ok(());
    });

    tracing::info!("processing {:?}, event=removed", path);
    notify(format!("removed device: {}", device).as_str())?;

    let dest: std::path::PathBuf = workingdir.join(PathBuf::from(device));
    tracing::debug!("destpath is {:?}", dest);

    if let Err(why) = unmount(&dest, UnmountFlags::FORCE) {
        tracing::error!("failed to unmount {:?}: {}", path, why);
    }

    remove_dir(dest).await?;
    Ok(())
}

fn enable_logging(with_extra_debug: bool) {
    let subscriber = tracing_subscriber::registry().with(tracing_subscriber::EnvFilter::new(
        std::env::var("RUST_LOG").unwrap_or_else(|_| "debug".into()),
    ));
    if with_extra_debug {
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
}

/// Yo, main body what do you expect. It does whatever usually main does.
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // ### logging setup
    println!("\n\nset DEBUGMODE to anything for easier debugging\n\n");
    let is_debug_mode = std::env::var("DEBUGMODE").map_or(false, |x| !x.is_empty());
    enable_logging(is_debug_mode);

    // ### preparations
    let settings = Config::builder()
        .add_source(config::Environment::with_prefix("RUDEVIL").separator("_"))
        .build()
        .unwrap();

    notify("rudevil is running! <3")?;
    tracing::info!("config is {:?}", &settings);

    // lets default here
    let wanteduser: String = settings.get("user").unwrap_or_else(|_| "root".to_owned());
    let wantedgroup: String = settings
        .get("group")
        .unwrap_or_else(|_| "plugdev".to_owned());
    let wantedworkidr: String = settings
        .get("workdir")
        .unwrap_or_else(|_| "/storage".to_owned());

    // lets check the creds
    let user_id = find_uid(wanteduser.to_owned()).await?;
    let group_id = find_gid(wantedgroup.to_owned()).await?;
    let workdir = find_workir(wantedworkidr.to_owned()).await?;

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
                process_created(path, user_id, group_id, workdir.clone()).await?;
            }
            Ok(DebouncedEvent::Remove(path)) => {
                process_removed(path, workdir.clone()).await?;
            }
            Err(e) => tracing::error!("watch error: {:?}", e),
            _ => {} // do nothing on unsupported events,
        }
    }
}
