#include <windows.h>
#include <shellapi.h>
#include <shlobj.h>

#include <getopt.h>

#include <algorithm>
#include <cctype>
#include <cstdint>
#include <exception>
#include <iomanip>
#include <iostream>
#include <iterator>
#include <sstream>
#include <stdexcept>
#include <string>
#include <unordered_set>
#include <utility>
#include <vector>

namespace {

constexpr int OPT_STATUS = 1000;
constexpr int OPT_EMPTY = 1001;
constexpr int OPT_NO_SOUND = 1002;
constexpr int OPT_CONFIRM_EMPTY = 1003;

class RecycleBinError : public std::runtime_error {
public:
    RecycleBinError(const std::string& message, std::uint32_t code = 1)
        : std::runtime_error(message), code_(code) {}

    std::uint32_t code() const noexcept { return code_; }

private:
    std::uint32_t code_;
};

struct RecycleBinStatus {
    unsigned long long size_bytes = 0;
    unsigned long long items = 0;
};

struct Options {
    bool show_help = false;
    bool status = false;
    bool empty = false;
    bool no_sound = false;
    bool confirm_empty = false;
    int verbose = 0;
    std::vector<std::string> paths;
};

std::wstring utf8_to_wide(const std::string& input) {
    if (input.empty()) {
        return std::wstring();
    }

    int size = MultiByteToWideChar(CP_UTF8, MB_ERR_INVALID_CHARS, input.data(),
                                   static_cast<int>(input.size()), nullptr, 0);
    UINT code_page = CP_UTF8;
    DWORD flags = MB_ERR_INVALID_CHARS;

    if (size == 0) {
        // MSYS2 UCRT64 normally passes UTF-8 argv. Fall back to the active ANSI
        // code page only for environments that do not follow that convention.
        code_page = CP_ACP;
        flags = 0;
        size = MultiByteToWideChar(code_page, flags, input.data(),
                                   static_cast<int>(input.size()), nullptr, 0);
    }

    if (size == 0) {
        throw RecycleBinError("failed to convert an argument to UTF-16", GetLastError());
    }

    std::wstring result(static_cast<std::size_t>(size), L'\0');
    int written = MultiByteToWideChar(code_page, flags, input.data(),
                                      static_cast<int>(input.size()), result.data(), size);
    if (written == 0) {
        throw RecycleBinError("failed to convert an argument to UTF-16", GetLastError());
    }
    return result;
}

std::string wide_to_utf8(const std::wstring& input) {
    if (input.empty()) {
        return std::string();
    }

    int size = WideCharToMultiByte(CP_UTF8, 0, input.data(),
                                   static_cast<int>(input.size()), nullptr, 0, nullptr, nullptr);
    if (size == 0) {
        return std::string();
    }

    std::string result(static_cast<std::size_t>(size), '\0');
    int written = WideCharToMultiByte(CP_UTF8, 0, input.data(),
                                      static_cast<int>(input.size()), result.data(), size,
                                      nullptr, nullptr);
    if (written == 0) {
        return std::string();
    }
    return result;
}

std::string windows_message(std::uint32_t code) {
    wchar_t* raw = nullptr;
    DWORD flags = FORMAT_MESSAGE_ALLOCATE_BUFFER | FORMAT_MESSAGE_FROM_SYSTEM |
                  FORMAT_MESSAGE_IGNORE_INSERTS;
    DWORD length = FormatMessageW(flags, nullptr, static_cast<DWORD>(code), 0,
                                  reinterpret_cast<LPWSTR>(&raw), 0, nullptr);

    if (length == 0 && HRESULT_FACILITY(static_cast<HRESULT>(code)) == FACILITY_WIN32) {
        length = FormatMessageW(flags, nullptr, HRESULT_CODE(static_cast<HRESULT>(code)), 0,
                                reinterpret_cast<LPWSTR>(&raw), 0, nullptr);
    }

    if (length == 0 || raw == nullptr) {
        return std::string();
    }

    std::wstring message(raw, raw + length);
    LocalFree(raw);

    while (!message.empty() && (message.back() == L'\r' || message.back() == L'\n' ||
                                message.back() == L' ' || message.back() == L'\t')) {
        message.pop_back();
    }

    return wide_to_utf8(message);
}

std::string hex_code(std::uint32_t code) {
    std::ostringstream out;
    out << "0x" << std::hex << std::uppercase << code;
    return out.str();
}

std::string operation_error(const std::string& operation, std::uint32_t code) {
    std::ostringstream out;
    out << operation << " failed (code=" << hex_code(code) << ")";

    std::string message = windows_message(code);
    if (!message.empty()) {
        out << "; " << message;
    }

    return out.str();
}

std::wstring full_path_no_expand(const std::wstring& path) {
    DWORD needed = GetFullPathNameW(path.c_str(), 0, nullptr, nullptr);
    if (needed == 0) {
        throw RecycleBinError(operation_error("Normalizing path", GetLastError()), GetLastError());
    }

    std::wstring buffer(static_cast<std::size_t>(needed), L'\0');
    DWORD written = GetFullPathNameW(path.c_str(), needed, buffer.data(), nullptr);
    if (written == 0) {
        throw RecycleBinError(operation_error("Normalizing path", GetLastError()), GetLastError());
    }
    if (written >= needed) {
        buffer.assign(static_cast<std::size_t>(written + 1), L'\0');
        written = GetFullPathNameW(path.c_str(), written + 1, buffer.data(), nullptr);
        if (written == 0) {
            throw RecycleBinError(operation_error("Normalizing path", GetLastError()), GetLastError());
        }
    }

    buffer.resize(written);
    return buffer;
}

std::vector<std::wstring> normalize_paths_no_expand(const std::vector<std::string>& paths) {
    std::vector<std::wstring> normalized;
    std::unordered_set<std::wstring> seen;

    for (const std::string& path : paths) {
        std::wstring wide_path = utf8_to_wide(path);
        std::wstring full_path = full_path_no_expand(wide_path);
        if (seen.insert(full_path).second) {
            normalized.push_back(std::move(full_path));
        }
    }

    return normalized;
}

std::pair<int, bool> delete_to_recycle_bin(const std::vector<std::string>& paths,
                                           FILEOP_FLAGS flags) {
    std::vector<std::wstring> normalized = normalize_paths_no_expand(paths);
    if (normalized.empty()) {
        return {0, false};
    }

    std::wstring from;
    for (const std::wstring& path : normalized) {
        from.append(path);
        from.push_back(L'\0');
    }
    from.push_back(L'\0');

    SHFILEOPSTRUCTW op{};
    op.hwnd = nullptr;
    op.wFunc = FO_DELETE;
    op.pFrom = from.c_str();
    op.pTo = nullptr;
    op.fFlags = flags;
    op.fAnyOperationsAborted = FALSE;
    op.hNameMappings = nullptr;
    op.lpszProgressTitle = nullptr;

    int result = SHFileOperationW(&op);
    return {result, op.fAnyOperationsAborted != FALSE};
}

bool send_to_recycle_bin(const std::vector<std::string>& paths,
                         bool allow_undo = true,
                         bool suppress_confirmation = true) {
    FILEOP_FLAGS flags = 0;
    if (allow_undo) {
        flags |= FOF_ALLOWUNDO;
    }
    if (suppress_confirmation) {
        flags |= FOF_NOCONFIRMATION;
    }

    auto [retcode, aborted] = delete_to_recycle_bin(paths, flags);
    if (retcode != 0) {
        throw RecycleBinError(operation_error("Moving files to the Recycle Bin",
                                             static_cast<std::uint32_t>(retcode)),
                              static_cast<std::uint32_t>(retcode));
    }

    return !aborted;
}

RecycleBinStatus get_recycle_bin_status() {
    SHQUERYRBINFO info{};
    info.cbSize = sizeof(info);

    HRESULT hr = SHQueryRecycleBinW(nullptr, &info);
    if (FAILED(hr)) {
        throw RecycleBinError(operation_error("Querying the Recycle Bin",
                                             static_cast<std::uint32_t>(hr)),
                              static_cast<std::uint32_t>(hr));
    }

    return {static_cast<unsigned long long>(info.i64Size),
            static_cast<unsigned long long>(info.i64NumItems)};
}

void empty_recycle_bin_with_options(bool require_confirmation,
                                    bool show_progress,
                                    bool play_sound) {
    DWORD flags = 0;
    if (!require_confirmation) {
        flags |= SHERB_NOCONFIRMATION;
    }
    if (!show_progress) {
        flags |= SHERB_NOPROGRESSUI;
    }
    if (!play_sound) {
        flags |= SHERB_NOSOUND;
    }

    HRESULT hr = SHEmptyRecycleBinW(nullptr, nullptr, flags);
    if (FAILED(hr)) {
        throw RecycleBinError(operation_error("Emptying the Recycle Bin",
                                             static_cast<std::uint32_t>(hr)),
                              static_cast<std::uint32_t>(hr));
    }
}

std::string human_size(unsigned long long bytes) {
    static constexpr const char* units[] = {"B", "KB", "MB", "GB", "TB", "PB"};
    double value = static_cast<double>(bytes);
    std::size_t unit_index = 0;

    while (value >= 1024.0 && unit_index + 1 < std::size(units)) {
        value /= 1024.0;
        ++unit_index;
    }

    std::ostringstream out;
    out << std::fixed << std::setprecision(1) << value << units[unit_index];
    return out.str();
}

void print_usage(const char* program_name) {
    std::cout
        << "Usage:\n"
        << "  " << program_name << " [PATH ...]\n"
        << "  " << program_name << " --status\n"
        << "  " << program_name << " --empty [--confirm-empty] [--no-sound]\n"
        << "\n"
        << "Options:\n"
        << "  --status          Show Recycle Bin total size and item count.\n"
        << "  --empty           Empty the Recycle Bin.\n"
        << "  --confirm-empty   Prompt in the terminal before emptying.\n"
        << "  --no-sound        Suppress the Recycle Bin empty sound.\n"
        << "  -v, --verbose     Print additional status messages. Can be repeated.\n"
        << "  -h, --help        Show this help message.\n"
        << "\n"
        << "PATH values are made absolute and normalized, but environment variables, '~',\n"
        << "and wildcards are not expanded by rmtrash.\n";
}

Options parse_options(int argc, char* argv[]) {
    Options options;

    static option long_options[] = {
        {"status", no_argument, nullptr, OPT_STATUS},
        {"empty", no_argument, nullptr, OPT_EMPTY},
        {"no-sound", no_argument, nullptr, OPT_NO_SOUND},
        {"confirm-empty", no_argument, nullptr, OPT_CONFIRM_EMPTY},
        {"verbose", no_argument, nullptr, 'v'},
        {"help", no_argument, nullptr, 'h'},
        {nullptr, 0, nullptr, 0},
    };

    opterr = 0;
    optind = 1;

    while (true) {
        int option_index = 0;
        int c = getopt_long(argc, argv, "hv", long_options, &option_index);
        if (c == -1) {
            break;
        }

        switch (c) {
        case OPT_STATUS:
            options.status = true;
            break;
        case OPT_EMPTY:
            options.empty = true;
            break;
        case OPT_NO_SOUND:
            options.no_sound = true;
            break;
        case OPT_CONFIRM_EMPTY:
            options.confirm_empty = true;
            break;
        case 'v':
            ++options.verbose;
            break;
        case 'h':
            options.show_help = true;
            break;
        case '?': {
            std::string opt = (optind > 0 && optind <= argc) ? argv[optind - 1] : "";
            if (opt.empty()) {
                throw std::invalid_argument("unknown option");
            }
            throw std::invalid_argument("unknown option: " + opt);
        }
        default:
            throw std::invalid_argument("failed to parse command-line options");
        }
    }

    for (int i = optind; i < argc; ++i) {
        options.paths.emplace_back(argv[i]);
    }

    return options;
}

bool confirm_empty() {
    std::cout << "Empty the Recycle Bin now? This cannot be undone. [y/N]: ";
    std::string response;
    std::getline(std::cin, response);

    auto is_not_space = [](unsigned char ch) { return !std::isspace(ch); };
    response.erase(response.begin(),
                   std::find_if(response.begin(), response.end(), is_not_space));
    response.erase(std::find_if(response.rbegin(), response.rend(), is_not_space).base(),
                   response.end());

    std::transform(response.begin(), response.end(), response.begin(),
                   [](unsigned char ch) { return static_cast<char>(std::tolower(ch)); });

    return response == "y" || response == "yes";
}

}  // namespace

int main(int argc, char* argv[]) {
    SetConsoleOutputCP(CP_UTF8);

    Options options;
    try {
        options = parse_options(argc, argv);
    } catch (const std::invalid_argument& exc) {
        std::cerr << exc.what() << "\n\n";
        print_usage(argv[0]);
        return 2;
    }

    if (options.show_help) {
        print_usage(argv[0]);
        return 0;
    }

    if (!options.status && !options.empty && options.paths.empty()) {
        print_usage(argv[0]);
        return 0;
    }

    try {
        if (options.status) {
            RecycleBinStatus status = get_recycle_bin_status();
            std::cout << "Recycle Bin status:\n"
                      << "  items : " << status.items << "\n"
                      << "  size  : " << status.size_bytes << " bytes ("
                      << human_size(status.size_bytes) << ")\n";
        }

        if (options.empty) {
            if (options.confirm_empty && !confirm_empty()) {
                std::cout << "Aborted by user.\n";
                return 0;
            }

            empty_recycle_bin_with_options(options.confirm_empty, false, !options.no_sound);
            std::cout << "Recycle Bin emptied.\n";
        }

        if (!options.paths.empty()) {
            bool completed = send_to_recycle_bin(options.paths, true, true);
            if (options.verbose > 0) {
                if (completed) {
                    std::cout << "Moved to recycle bin:";
                    for (const std::string& path : options.paths) {
                        std::cout << ' ' << path;
                    }
                    std::cout << "\n";
                } else {
                    std::cout << "Recycle Bin operation was aborted by the shell.\n";
                }
            }
        }
    } catch (const RecycleBinError& exc) {
        std::cerr << exc.what() << '\n';
        return 1;
    } catch (const std::exception& exc) {
        std::cerr << exc.what() << '\n';
        return 1;
    }

    return 0;
}
