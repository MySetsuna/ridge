//! OS evidence for exact process instances and local protocol socket ownership.
//! No executable-name/brand inference; no process launch or termination.
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, io, net::SocketAddrV4};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessInstance {
    pub pid: u32,
    pub start_instance: String,
}

pub fn instance(pid: u32) -> io::Result<ProcessInstance> {
    if pid == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "process PID must be nonzero",
        ));
    }
    Ok(ProcessInstance {
        pid,
        start_instance: platform::start(pid)?,
    })
}
pub fn validate(expected: &ProcessInstance) -> io::Result<()> {
    if instance(expected.pid)? != *expected {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "process instance changed",
        ));
    }
    Ok(())
}
pub fn descendant(root: &ProcessInstance, target: &ProcessInstance) -> io::Result<bool> {
    validate(root)?;
    validate(target)?;
    let mut current = target.pid;
    let mut visited = HashSet::new();
    while visited.insert(current) && visited.len() <= 256 {
        if current == root.pid {
            validate(root)?;
            validate(target)?;
            return Ok(true);
        }
        let Ok(Some(parent)) = platform::parent(current) else {
            return Ok(false);
        };
        // Parent creation must precede child creation; a recycled PPID is not ancestry.
        let (Ok(parent_start), Ok(child_start)) =
            (platform::start(parent), platform::start(current))
        else {
            return Ok(false);
        };
        if parent_start > child_start {
            return Ok(false);
        }
        current = parent;
    }
    Ok(false)
}
pub fn owns_connection(
    owner: &ProcessInstance,
    server: SocketAddrV4,
    client: SocketAddrV4,
) -> io::Result<()> {
    if !server.ip().is_loopback() || !client.ip().is_loopback() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "ACP OS-attested connection must be loopback",
        ));
    }
    validate(owner)?;
    if !platform::owns(owner.pid, server, client)? {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "protocol socket belongs to a different process",
        ));
    }
    validate(owner)
}

#[cfg(windows)]
mod platform {
    use super::*;
    use std::ffi::c_void;
    #[repr(C)]
    #[derive(Default)]
    struct FileTime {
        low: u32,
        high: u32,
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut c_void;
        fn CloseHandle(handle: *mut c_void) -> i32;
        fn GetProcessTimes(
            handle: *mut c_void,
            created: *mut FileTime,
            exited: *mut FileTime,
            kernel: *mut FileTime,
            user: *mut FileTime,
        ) -> i32;
        fn WaitForSingleObject(handle: *mut c_void, timeout: u32) -> u32;
    }
    #[link(name = "iphlpapi")]
    extern "system" {
        fn GetExtendedTcpTable(
            table: *mut c_void,
            size: *mut u32,
            ordered: i32,
            family: u32,
            class: u32,
            reserved: u32,
        ) -> u32;
    }
    struct Handle(*mut c_void);
    impl Drop for Handle {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
    pub fn start(pid: u32) -> io::Result<String> {
        let handle = Handle(unsafe { OpenProcess(0x00101000, 0, pid) });
        if handle.0.is_null() {
            let error = io::Error::last_os_error();
            return Err(if error.raw_os_error() == Some(87) {
                io::Error::new(io::ErrorKind::NotFound, "process no longer exists")
            } else {
                error
            });
        }
        let (mut created, mut exited, mut kernel, mut user) = (
            FileTime::default(),
            FileTime::default(),
            FileTime::default(),
            FileTime::default(),
        );
        if unsafe { GetProcessTimes(handle.0, &mut created, &mut exited, &mut kernel, &mut user) }
            == 0
        {
            return Err(io::Error::last_os_error());
        }
        let wait = unsafe { WaitForSingleObject(handle.0, 0) };
        if wait != 258 {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("process {pid} is not running (wait status {wait})"),
            ));
        }
        Ok(format!(
            "win:{:016x}",
            ((created.high as u64) << 32) | created.low as u64
        ))
    }
    pub fn parent(pid: u32) -> io::Result<Option<u32>> {
        let mut system = sysinfo::System::new();
        system.refresh_processes(sysinfo::ProcessesToUpdate::Some(&[sysinfo::Pid::from_u32(
            pid,
        )]));
        let process = system
            .process(sysinfo::Pid::from_u32(pid))
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "process not found"))?;
        Ok(process.parent().map(|pid| pid.as_u32()))
    }
    pub fn owns(pid: u32, server: SocketAddrV4, client: SocketAddrV4) -> io::Result<bool> {
        let mut size = 0;
        let first = unsafe { GetExtendedTcpTable(std::ptr::null_mut(), &mut size, 0, 2, 5, 0) };
        if first != 0 && first != 122 {
            return Err(io::Error::from_raw_os_error(first as i32));
        }
        for _ in 0..4 {
            if size > 64 * 1024 * 1024 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "TCP owner table exceeds budget",
                ));
            }
            let mut table = vec![0u32; (size as usize + 3) / 4];
            let result =
                unsafe { GetExtendedTcpTable(table.as_mut_ptr().cast(), &mut size, 0, 2, 5, 0) };
            if result == 122 {
                continue;
            }
            if result != 0 {
                return Err(io::Error::from_raw_os_error(result as i32));
            }
            let count = table.first().copied().unwrap_or(0) as usize;
            if count
                .checked_mul(6)
                .and_then(|n| n.checked_add(1))
                .is_none_or(|n| n > table.len())
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "invalid TCP owner table",
                ));
            }
            let owners: HashSet<_> = table[1..1 + count * 6]
                .chunks_exact(6)
                .filter(|row| {
                    row[0] == 5
                        && row[1] == u32::from_ne_bytes(server.ip().octets())
                        && u16::from_be(row[2] as u16) == server.port()
                        && row[3] == u32::from_ne_bytes(client.ip().octets())
                        && u16::from_be(row[4] as u16) == client.port()
                })
                .map(|row| row[5])
                .collect();
            return Ok(owners.len() == 1 && owners.contains(&pid));
        }
        Err(io::Error::new(
            io::ErrorKind::WouldBlock,
            "TCP owner table changed repeatedly",
        ))
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use super::*;
    fn stat(pid: u32) -> io::Result<Vec<String>> {
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat"))?;
        let end = stat
            .rfind(')')
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid process stat"))?;
        let fields: Vec<_> = stat[end + 1..]
            .split_whitespace()
            .map(str::to_owned)
            .collect();
        if fields.len() < 20 || fields[0] == "Z" {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "process unavailable",
            ));
        }
        Ok(fields)
    }
    pub fn start(pid: u32) -> io::Result<String> {
        let fields = stat(pid)?;
        let ticks = fields[19].parse::<u64>().map_err(|_| {
            io::Error::new(io::ErrorKind::InvalidData, "invalid process start ticks")
        })?;
        let boot = std::fs::read_to_string("/proc/sys/kernel/random/boot_id")?;
        Ok(format!("linux:{}:{ticks:016x}", boot.trim()))
    }
    pub fn parent(pid: u32) -> io::Result<Option<u32>> {
        let pid = stat(pid)?[1]
            .parse::<u32>()
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid parent PID"))?;
        Ok((pid != 0).then_some(pid))
    }
    fn address(value: &str) -> Option<SocketAddrV4> {
        let (ip, port) = value.split_once(':')?;
        Some(SocketAddrV4::new(
            std::net::Ipv4Addr::from(u32::from_str_radix(ip, 16).ok()?.to_ne_bytes()),
            u16::from_str_radix(port, 16).ok()?,
        ))
    }
    pub fn owns(pid: u32, server: SocketAddrV4, client: SocketAddrV4) -> io::Result<bool> {
        let table = std::fs::read_to_string("/proc/net/tcp")?;
        let inodes: HashSet<_> = table
            .lines()
            .skip(1)
            .filter_map(|line| {
                let fields: Vec<_> = line.split_whitespace().collect();
                if fields.len() > 9
                    && fields[3] == "01"
                    && address(fields[1]) == Some(server)
                    && address(fields[2]) == Some(client)
                {
                    Some(format!("socket:[{}]", fields[9]))
                } else {
                    None
                }
            })
            .collect();
        for fd in std::fs::read_dir(format!("/proc/{pid}/fd"))? {
            if let Ok(target) = std::fs::read_link(fd?.path()) {
                if inodes.contains(target.to_string_lossy().as_ref()) {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }
}
#[cfg(not(any(windows, target_os = "linux")))]
mod platform {
    use super::*;
    fn unsupported() -> io::Error {
        io::Error::new(
            io::ErrorKind::Unsupported,
            "OS process/socket attestation unavailable on this platform",
        )
    }
    pub fn start(_: u32) -> io::Result<String> {
        Err(unsupported())
    }
    pub fn parent(_: u32) -> io::Result<Option<u32>> {
        Err(unsupported())
    }
    pub fn owns(_: u32, _: SocketAddrV4, _: SocketAddrV4) -> io::Result<bool> {
        Err(unsupported())
    }
}

#[cfg(all(test, any(windows, target_os = "linux")))]
mod tests {
    use super::*;
    use std::process::{Command, Stdio};
    struct Child(std::process::Child);
    impl Drop for Child {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    #[test]
    fn process_instance_and_socket_owner_are_os_facts() {
        let current = instance(std::process::id()).unwrap();
        assert_eq!(instance(current.pid).unwrap(), current);
        let mut command = Command::new(std::env::current_exe().unwrap());
        command.args([
            "--exact",
            "process_identity::tests::process_identity_child_fixture",
            "--ignored",
        ]);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        let mut child = Child(
            command
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
        );
        let child_instance = instance(child.0.id()).unwrap_or_else(|error| {
            panic!(
                "child identity failed: {error}; child status: {:?}",
                child.0.try_wait()
            )
        });
        assert!(descendant(&current, &child_instance).unwrap());
        assert!(!descendant(&child_instance, &current).unwrap());
        let mut changed = child_instance.clone();
        changed.start_instance.push('x');
        assert!(validate(&changed).is_err());
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let socket = std::net::TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (server, _) = listener.accept().unwrap();
        let std::net::SocketAddr::V4(local) = server.local_addr().unwrap() else {
            unreachable!()
        };
        let std::net::SocketAddr::V4(remote) = socket.local_addr().unwrap() else {
            unreachable!()
        };
        owns_connection(&current, local, remote).unwrap();
        assert!(owns_connection(&child_instance, local, remote).is_err());
        child.0.kill().unwrap();
        child.0.wait().unwrap();
        assert!(validate(&child_instance).is_err());
    }

    #[test]
    #[ignore = "owned child-process fixture"]
    fn process_identity_child_fixture() {
        std::thread::sleep(std::time::Duration::from_secs(30));
    }
}
