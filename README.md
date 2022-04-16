# rudevil
Simple automount daemon for linux.


### What is that

For servers adn userless machines it would be niceto be able to mount devices without user interaction. This tool serves exactly that purpose.

### ok, but how?

Run it as a systemd service. It has to be started by root because linux reasons.

### as root? is it secure

... maybe ?

### can user use the mount ?

Yes
intended and default mode is for user to be in `plugdev` mode. any `/dev/sdXY` will be mounted (by default) to `/storage/sdXY` and forcibly unmounted if lost so there should be no trash lying around. HOWEVER if the state is already trashy, manual cleanup and unmounting is needed. Rudvil does not expect the directory to be empty, but there should be neither directories nor mountpoints matching `sd[a-z]{1,2}[0-9]{1,2}` in the destination directory, or expect errors.

there are only few errors that result in crash: can't find user/group/workdir, or permissions error.
In first case, rtfm, in second run as root.
... great, that was easiest troubleshooting ever.

### But i run ubuntu, will it work there?
it should, if it does not, report an issue

### is systemd unit provided ?
not yet

### is there installer ?
no, so far not planned.
idc really, i'm running gentoo, we don't do installers herem, those are gey.

### can we commit
sure, why not, but start issue first and lets discuss the feature.
I will do that for my own feats too

### contact:

<esvi at pm dot me>
anything else goes automatically to spam;
