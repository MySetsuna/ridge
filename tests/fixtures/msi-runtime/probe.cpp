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

static UINT trace(MSIHANDLE session, const char* stage, bool deferred = false) {
    auto root = property(session, deferred ? L"CustomActionData" : L"RIDGE_PROBE_ROOT");
    if (root.empty()) return ERROR_INSTALL_FAILURE;
    auto target = root + L"\\installed\\sentinel.txt";
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

extern "C" __declspec(dllexport) UINT __stdcall ProbeBefore(MSIHANDLE s) { return trace(s, "before"); }
extern "C" __declspec(dllexport) UINT __stdcall ProbeWritten(MSIHANDLE s) { return trace(s, "written", true); }
extern "C" __declspec(dllexport) UINT __stdcall ProbeFailure(MSIHANDLE s) { trace(s, "fail", true); return ERROR_INSTALL_FAILURE; }
extern "C" __declspec(dllexport) UINT __stdcall ProbeCancel(MSIHANDLE s) { trace(s, "cancel", true); return ERROR_INSTALL_USEREXIT; }
extern "C" __declspec(dllexport) UINT __stdcall ProbeSuccess(MSIHANDLE s) { return trace(s, "exit-success"); }
extern "C" __declspec(dllexport) UINT __stdcall ProbeError(MSIHANDLE s) { return trace(s, "exit-error"); }
extern "C" __declspec(dllexport) UINT __stdcall ProbeUserExit(MSIHANDLE s) { return trace(s, "exit-cancel"); }
extern "C" __declspec(dllexport) UINT __stdcall ProbeSuspend(MSIHANDLE s) { return trace(s, "exit-suspend"); }
