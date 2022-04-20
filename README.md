
# rudevil
Simple automount daemon for linux.

tested with:
- btrfs,ext4 - works just fine
- vfat - does not work, this is kind of problematic, vfat does not support ownership, and is problematic to mount. From my experiments its broken. Fixing it will require adding changes to the mount library we are using which is planned. but will depend on the amount of time we have for it.

### What is that
For servers and userless machines, it would be nice to be able to mount devices without user interaction. This tool serves exactly that purpose.

### ok, but how?
Run it as a systemd service. It has to be started by root because linux reasons. And yes, initrc and all other flavors of init scripting also should work as long as you are able to pass some environment variables. Currently, there is no config, so the daemon is fully autonomous. Remember to either disable suid or protect it from users. You are responsible for your own security, i did what i could. 

### configs:
env only for now:
- `RUDEVIL_USER` - defaults to "root" - user used for mounting (ownership of mount dir)
- `RUDEVIL_GROUP` - defaults to "plugdev" - group used for mounting (ownership of mount dir)
- `RUDEVIL_WORKDIR` - defaults to "/storage" - default mounting root directory 

### ...as root? Is it secure
... should be ?
linux do not really like to allow other people mounting drives since it can result in privilege escalations. That's why your safety is on you and only you. You have to be sure what you are doing and how, its not my responsibility to take care of your setup. 
Rules of the daemon are simple.
- all mounts are in the specified directory and nowhere else, 
- if you need one user to be owner of the mount you can specify it, otherwise use group id or add user to `plugdev` which is default
- only block devices recognized as `/dev/sdXY` are mounted.

### so... can user use the mount ?
Yes, that was intended. 
intended and default mode is for user to be in `plugdev` mode. Any `/dev/sdXY` will be mounted (by default) to `/storage/sdXY` and forcibly unmounted if lost, so there should be no trash lying around. HOWEVER, if the state is already trashy, manual cleanup and unmounting is needed. Rudvil does not expect the directory to be empty, but there should be neither directories nor mount points matching `sd[a-z]{1,2}[0-9]{1,2}` in the destination directory, or expect errors.

Trashy state means someone mounted /dev/sdx1 (or via rudevil) and unplugged the device without unmounting (or rudevil crashed before it could handle cleanup). That mount (/dev/sdx1 in /storage/sdx1) will not be cleaned by rudevil before mounting and will result in mount error (not crash)

Be mindfull that rudevil does not handle root devices (i.e. `/dev/sdX` without a number ). I know, i use some of my devices like that but most people don't and usage as `sdXY` would be in 95% correct one.
This may change in the future but its okay for now. This also means that luks devices are not handled as well - and that also may or may not change in the future, we will see...

there are only few errors that result in crash: can't find user/group/workdir, or permissions error.
In first case, rtfm, in second run as root.
... great, that was easiest troubleshooting ever.

### But i run ubuntu, will it work there?
it should, if it does not, report an issue

... also you should stop and install gentoo

### is systemd unit provided ?
yes, in the `systemd` folder.
Remember to change the user/group/workdir and point the service to wherever the binary is.


### is there installer ?
no, so far not planned.
idc really, i'm running Gentoo, we don't do installers here, those are gey.

### can we commit
sure, why not, but start issue first and lets discuss the feature.
I will do that for my own feats too. I will try to make the process as easy as possible,
Also, you have to fork unless you are part of the organization, obviously...

### contact:

"esvi at pm dot me"
anything else goes automatically to spam;
