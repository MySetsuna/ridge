// Actual Windows Installer lifecycle evidence; never linked into Ridge.
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <msiquery.h>
#include <sddl.h>
#include <tlhelp32.h>
#include <string>
#include <vector>

static std::wstring property(MSIHANDLE session, const wchar_t* name) {
    DWORD size = 0;
    wchar_t empty = 0;
    UINT result = MsiGetPropertyW(session, name, &empty, &size);
    if (result != ERROR_MORE_DATA && result != ERROR_SUCCESS) return L"";
    std::vector<wchar_t> value(size + 1);
    ++size;
    if (MsiGetPropertyW(session, name, value.data(), &size) != ERROR_SUCCESS) return L"";
    return value.data();
}

static std::string quoted(const std::wstring& value) {
    int count = WideCharToMultiByte(CP_UTF8, 0, value.data(), static_cast<int>(value.size()), nullptr, 0, nullptr, nullptr);
    std::string utf8(count, '\0');
    if (count) WideCharToMultiByte(CP_UTF8, 0, value.data(), static_cast<int>(value.size()), utf8.data(), count, nullptr, nullptr);
    std::string out = "\"";
    for (unsigned char ch : utf8) {
        if (ch == '\\' || ch == '"') out += '\\';
        if (ch < 0x20) {
            const char hex[] = "0123456789abcdef";
            out += "\\u00"; out += hex[ch >> 4]; out += hex[ch & 15];
        } else out += static_cast<char>(ch);
    }
    return out + "\"";
}

static std::wstring token_sid(HANDLE token) {
    DWORD size = 0;
    GetTokenInformation(token, TokenUser, nullptr, 0, &size);
    std::vector<BYTE> data(size);
    if (!size || !GetTokenInformation(token, TokenUser, data.data(), size, &size)) return L"";
    LPWSTR sid = nullptr;
    if (!ConvertSidToStringSidW(reinterpret_cast<TOKEN_USER*>(data.data())->User.Sid, &sid)) return L"";
    std::wstring value(sid);
    LocalFree(sid);
    return value;
}

static std::wstring process_sid() {
    HANDLE token = nullptr;
    if (!OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &token)) return L"";
    auto sid = token_sid(token); CloseHandle(token); return sid;
}

static std::wstring thread_sid() {
    HANDLE token = nullptr;
    if (!OpenThreadToken(GetCurrentThread(), TOKEN_QUERY, TRUE, &token)) return L"";
    auto sid = token_sid(token); CloseHandle(token); return sid;
}

static DWORD parent_pid(DWORD pid) {
    HANDLE snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
    if (snapshot == INVALID_HANDLE_VALUE) return 0;
    PROCESSENTRY32W entry{}; entry.dwSize = sizeof(entry);
    DWORD parent = 0;
    if (Process32FirstW(snapshot, &entry)) do {
        if (entry.th32ProcessID == pid) { parent = entry.th32ParentProcessID; break; }
    } while (Process32NextW(snapshot, &entry));
    CloseHandle(snapshot);
    return parent;
}

static std::string process_identity(DWORD pid) {
    HANDLE process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, FALSE, pid);
    FILETIME created{}, exited{}, kernel{}, user{};
    wchar_t image[32768]{}; DWORD length = 32768;
    bool timed = process && GetProcessTimes(process, &created, &exited, &kernel, &user);
    bool named = process && QueryFullProcessImageNameW(process, 0, image, &length);
    if (process) CloseHandle(process);
    ULARGE_INTEGER stamp{}; stamp.LowPart = created.dwLowDateTime; stamp.HighPart = created.dwHighDateTime;
    return "{\"pid\":" + std::to_string(pid) + ",\"started\":" + (timed ? "\"" + std::to_string(stamp.QuadPart) + "\"" : "null")
        + ",\"image\":" + (named ? quoted(image) : "null") + "}";
}

static std::wstring read_status(const std::wstring& path) {
    HANDLE file = CreateFileW(path.c_str(), GENERIC_READ, FILE_SHARE_READ, nullptr, OPEN_EXISTING, 0, nullptr);
    if (file == INVALID_HANDLE_VALUE) return L"";
    char data[1024]{}; DWORD count = 0;
    bool ok = ReadFile(file, data, sizeof(data) - 1, &count, nullptr) != FALSE;
    CloseHandle(file);
    return ok ? std::wstring(data, data + count) : L"";
}

static HANDLE spawn_helper(MSIHANDLE session, const std::wstring& arguments) {
    auto helper = property(session, L"RIDGE_PROBE_HELPER");
    auto root = property(session, L"RIDGE_PROBE_ROOT");
    if (helper.empty() || root.empty()) return nullptr;
    LPWCH original = GetEnvironmentStringsW();
    if (!original) return nullptr;
    std::vector<wchar_t> environment;
    for (auto cursor = original; *cursor; cursor += wcslen(cursor) + 1) {
        if (_wcsnicmp(cursor, L"RIDGE_KERNEL_DATA_DIR=", 22) != 0)
            environment.insert(environment.end(), cursor, cursor + wcslen(cursor) + 1);
    }
    FreeEnvironmentStringsW(original);
    auto private_data = L"RIDGE_KERNEL_DATA_DIR=" + root + L"\\runtime";
    environment.insert(environment.end(), private_data.begin(), private_data.end());
    environment.push_back(0); environment.push_back(0);
    auto command = L"\"" + helper + L"\" " + arguments;
    STARTUPINFOW startup{}; startup.cb = sizeof(startup);
    PROCESS_INFORMATION process{};
    if (!CreateProcessW(helper.c_str(), command.data(), nullptr, nullptr, FALSE,
        CREATE_UNICODE_ENVIRONMENT | CREATE_NO_WINDOW | CREATE_BREAKAWAY_FROM_JOB,
        environment.data(), root.c_str(), &startup, &process)) return nullptr;
    CloseHandle(process.hThread);
    return process.hProcess;
}

static std::wstring handshake(MSIHANDLE session) {
    return property(session, L"RIDGE_PROBE_ROOT") + (property(session, L"REMOVE") == L"ALL" ? L"\\uninstall-runtime" : L"\\runtime");
}

static UINT reserve_helper(MSIHANDLE session) {
    auto stem = handshake(session);
    if (property(session, L"RIDGE_PROBE_HELPER").empty()) return ERROR_SUCCESS;
    // Observe the real execution-service ancestor, which outlives CA workers.
    DWORD parent = parent_pid(GetCurrentProcessId());
    if (!parent) return ERROR_INSTALL_FAILURE;
    auto arguments = L"--installer-parent " + std::to_wstring(parent)
        + L" --installer-status \"" + stem + L".status\" --installer-release \"" + stem + L".release\"";
    HANDLE child = spawn_helper(session, arguments);
    if (!child) return ERROR_INSTALL_FAILURE;
    UINT result = ERROR_INSTALL_FAILURE;
    for (int attempt = 0; attempt < 100; ++attempt) {
        auto status = read_status(stem + L".status");
        if (status == L"READY") { result = ERROR_SUCCESS; break; }
        if (!status.empty() || WaitForSingleObject(child, 0) == WAIT_OBJECT_0) break;
        Sleep(100);
    }
    CloseHandle(child);
    return result;
}

static UINT contend_helper(MSIHANDLE session) {
    if (property(session, L"RIDGE_PROBE_HELPER").empty()) return ERROR_SUCCESS;
    HANDLE child = spawn_helper(session, L"--installer-check");
    if (!child) return ERROR_INSTALL_FAILURE;
    DWORD exit = 0;
    bool rejected = WaitForSingleObject(child, 10000) == WAIT_OBJECT_0 && GetExitCodeProcess(child, &exit) && exit != 0;
    CloseHandle(child);
    return rejected ? ERROR_SUCCESS : ERROR_INSTALL_FAILURE;
}

static UINT release_helper(MSIHANDLE session) {
    auto stem = handshake(session);
    if (property(session, L"RIDGE_PROBE_HELPER").empty()) return ERROR_SUCCESS;
    auto status = read_status(stem + L".status");
    if (status != L"READY") return ERROR_INSTALL_FAILURE;
    auto release = stem + L".release";
    HANDLE file = CreateFileW(release.c_str(), GENERIC_WRITE, 0, nullptr, CREATE_NEW, 0, nullptr);
    if (file == INVALID_HANDLE_VALUE) return ERROR_INSTALL_FAILURE;
    CloseHandle(file);
    for (int attempt = 0; attempt < 100; ++attempt) {
        if (read_status(stem + L".done") == L"DONE") return ERROR_SUCCESS;
        Sleep(100);
    }
    return ERROR_INSTALL_FAILURE;
}

static UINT trace(MSIHANDLE session, const char* stage, bool deferred = false) {
    auto root = property(session, deferred ? L"CustomActionData" : L"RIDGE_PROBE_ROOT");
    if (root.empty()) return ERROR_INSTALL_FAILURE;
    auto target = root + L"\\installed\\sentinel.txt";
    if (deferred && read_status(root + L"\\runtime.status") == L"READY") {
        for (auto name : { L"kernel.boot.lock", L"kernel.lock" }) {
            auto lock_path = root + L"\\runtime\\" + name;
            HANDLE lock = CreateFileW(lock_path.c_str(), GENERIC_READ | GENERIC_WRITE,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE, nullptr, OPEN_EXISTING, 0, nullptr);
            if (lock == INVALID_HANDLE_VALUE) return ERROR_INSTALL_FAILURE;
            OVERLAPPED range{};
            bool acquired = LockFileEx(lock, LOCKFILE_EXCLUSIVE_LOCK | LOCKFILE_FAIL_IMMEDIATELY, 0, 1, 0, &range) != FALSE;
            DWORD error = GetLastError();
            if (acquired) UnlockFileEx(lock, 0, 1, 0, &range);
            CloseHandle(lock);
            if (acquired || error != ERROR_LOCK_VIOLATION) return ERROR_INSTALL_FAILURE;
        }
    }
    DWORD attributes = GetFileAttributesW(target.c_str());
    std::string line = "{\"stage\":\"" + std::string(stage) + "\",\"process\":" + process_identity(GetCurrentProcessId())
        + ",\"threadId\":" + std::to_string(GetCurrentThreadId()) + ",\"processSid\":" + quoted(process_sid())
        + ",\"threadSid\":" + quoted(thread_sid()) + ",\"uiLevel\":" + quoted(property(session, L"UILevel"))
        + ",\"userSid\":" + quoted(property(session, L"UserSID")) + ",\"localAppData\":" + quoted(property(session, L"LocalAppDataFolder"))
        + ",\"installedFileExists\":" + (attributes != INVALID_FILE_ATTRIBUTES ? "true" : "false") + ",\"ancestors\":[";
    DWORD pid = GetCurrentProcessId();
    for (int depth = 0; depth < 8; ++depth) {
        DWORD parent = parent_pid(pid);
        if (!parent || parent == pid) break;
        if (depth) line += ",";
        line += process_identity(parent); pid = parent;
    }
    line += "]}\n";
    auto path = root + L"\\trace.jsonl";
    HANDLE file = CreateFileW(path.c_str(), FILE_APPEND_DATA, FILE_SHARE_READ | FILE_SHARE_WRITE, nullptr, OPEN_ALWAYS, FILE_ATTRIBUTE_NORMAL, nullptr);
    if (file == INVALID_HANDLE_VALUE) return ERROR_INSTALL_FAILURE;
    DWORD written = 0;
    bool ok = WriteFile(file, line.data(), static_cast<DWORD>(line.size()), &written, nullptr) && written == line.size();
    FlushFileBuffers(file); CloseHandle(file);
    return ok ? ERROR_SUCCESS : ERROR_INSTALL_FAILURE;
}

extern "C" __declspec(dllexport) UINT __stdcall ProbeBefore(MSIHANDLE s) {
    UINT result = reserve_helper(s);
    return result == ERROR_SUCCESS ? trace(s, "before") : result;
}
extern "C" __declspec(dllexport) UINT __stdcall ProbeContend(MSIHANDLE s) { return contend_helper(s); }
extern "C" __declspec(dllexport) UINT __stdcall ProbeWritten(MSIHANDLE s) { return trace(s, "written", true); }
extern "C" __declspec(dllexport) UINT __stdcall ProbeFailure(MSIHANDLE s) { trace(s, "fail", true); return ERROR_INSTALL_FAILURE; }
extern "C" __declspec(dllexport) UINT __stdcall ProbeCancel(MSIHANDLE s) { trace(s, "cancel", true); return ERROR_INSTALL_USEREXIT; }
extern "C" __declspec(dllexport) UINT __stdcall ProbeCrash(MSIHANDLE s) {
    trace(s, "crash", true);
    // Kill only this disposable fixture's CA worker; the service must roll back.
    TerminateProcess(GetCurrentProcess(), 77);
    return ERROR_INSTALL_FAILURE;
}
static UINT finish(MSIHANDLE session, const char* stage) {
    UINT result = contend_helper(session);
    if (result == ERROR_SUCCESS) result = release_helper(session);
    return result == ERROR_SUCCESS ? trace(session, stage) : result;
}
extern "C" __declspec(dllexport) UINT __stdcall ProbeSuccess(MSIHANDLE s) { return finish(s, "exit-success"); }
extern "C" __declspec(dllexport) UINT __stdcall ProbeError(MSIHANDLE s) { return finish(s, "exit-error"); }
extern "C" __declspec(dllexport) UINT __stdcall ProbeUserExit(MSIHANDLE s) { return finish(s, "exit-cancel"); }
extern "C" __declspec(dllexport) UINT __stdcall ProbeSuspend(MSIHANDLE s) { return finish(s, "exit-suspend"); }
