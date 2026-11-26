"""Read-isolated native qualification on Linux with bubblewrap."""
import os
from pathlib import Path


def boundary(stage, read=(), executable=(), *, privileged_mounts=False):
    """Expose only system tools, selected inputs and one writable working stage."""
    if os.getuid() == 0:
        raise ValueError("Linux qualification must run as a regular user")
    prefix = ["/usr/bin/sudo", "-n"] if privileged_mounts else []
    prefix += ["/usr/bin/bwrap", "--unshare-net", "--unshare-pid", "--unshare-ipc",
               "--unshare-uts", "--die-with-parent", "--new-session", "--clearenv",
               "--setenv", "PATH", "/usr/bin:/bin", "--setenv", "HOME", str(stage),
               "--setenv", "TMPDIR", str(stage), "--setenv", "LANG", "C",
               "--setenv", "LC_ALL", "C", "--setenv", "TZ", "UTC"]
    # Bubblewrap creates implicit parent directories mode 0700. Root creates them
    # for privileged mounts, so the dropped child needs searchable empty ancestors.
    mount_inputs = [stage, *(path for path in (*read, *executable) if path is not None)]
    parents = {Path("/usr"), Path("/etc")}
    for path in mount_inputs:
        parents.update(parent for parent in Path(path).parents if parent != Path("/"))
    for parent in sorted(parents, key=lambda path: (len(path.parts), str(path))):
        prefix += ["--perms", "0755", "--dir", str(parent)]
    # Keep host home directories, /tmp, Cargo caches and /usr/local outside the namespace.
    for name in ("/usr/bin", "/usr/lib", "/usr/lib64", "/usr/libexec", "/etc/alternatives",
                 "/etc/ld.so.cache", "/bin", "/lib", "/lib64"):
        path = Path(name)
        if path.is_symlink():
            prefix += ["--symlink", os.readlink(path), name]
        elif path.exists():
            prefix += ["--ro-bind", name, name]
    prefix += ["--dev", "/dev", "--proc", "/proc"]
    for path in (*read, *executable):
        if path is not None:
            prefix += ["--ro-bind", str(path), str(path)]
    prefix += ["--bind", str(stage), str(stage), "--chdir", str(stage), "--"]
    if privileged_mounts:
        prefix += ["/usr/bin/setpriv", "--reuid", str(os.getuid()), "--regid", str(os.getgid()),
                   "--clear-groups", "--no-new-privs", "--"]
    return prefix
