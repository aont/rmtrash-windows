#include <windows.h>
#include <shellapi.h>
#include <shlobj.h>

#include <getopt.h>

#include <ctype.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdarg.h>
#include <wchar.h>

#define OPT_STATUS        1000
#define OPT_EMPTY         1001
#define OPT_NO_SOUND      1002
#define OPT_CONFIRM_EMPTY 1003

typedef struct RecycleBinStatus {
    unsigned long long size_bytes;
    unsigned long long items;
} RecycleBinStatus;

typedef struct Options {
    bool show_help;
    bool status;
    bool empty;
    bool no_sound;
    bool confirm_empty;
    int verbose;
    char** paths;
    int path_count;
} Options;

typedef struct WidePathList {
    wchar_t** items;
    size_t count;
    size_t capacity;
} WidePathList;

typedef struct FileOpResult {
    int retcode;
    bool aborted;
} FileOpResult;

static char* duplicate_string(const char* s) {
    size_t len;
    char* copy;

    if (s == NULL) {
        s = "";
    }

    len = strlen(s);
    copy = (char*)malloc(len + 1);
    if (copy == NULL) {
        return NULL;
    }

    memcpy(copy, s, len + 1);
    return copy;
}

static char* format_string(const char* fmt, ...) {
    va_list ap;
    va_list ap_copy;
    int needed;
    char* buffer;

    va_start(ap, fmt);
    va_copy(ap_copy, ap);
    needed = vsnprintf(NULL, 0, fmt, ap_copy);
    va_end(ap_copy);

    if (needed < 0) {
        va_end(ap);
        return duplicate_string("failed to format message");
    }

    buffer = (char*)malloc((size_t)needed + 1);
    if (buffer == NULL) {
        va_end(ap);
        return NULL;
    }

    vsnprintf(buffer, (size_t)needed + 1, fmt, ap);
    va_end(ap);
    return buffer;
}

static void set_error(char** error, char* message) {
    if (error == NULL) {
        free(message);
        return;
    }
    free(*error);
    *error = message;
}

static char* wide_to_utf8(const wchar_t* input) {
    int size;
    int written;
    char* result;

    if (input == NULL || input[0] == L'\0') {
        return duplicate_string("");
    }

    size = WideCharToMultiByte(CP_UTF8, 0, input, -1, NULL, 0, NULL, NULL);
    if (size == 0) {
        return duplicate_string("");
    }

    result = (char*)malloc((size_t)size);
    if (result == NULL) {
        return NULL;
    }

    written = WideCharToMultiByte(CP_UTF8, 0, input, -1, result, size, NULL, NULL);
    if (written == 0) {
        free(result);
        return duplicate_string("");
    }

    return result;
}

static int utf8_to_wide(const char* input, wchar_t** output, char** error, uint32_t* error_code) {
    int size;
    int written;
    UINT code_page = CP_UTF8;
    DWORD flags = MB_ERR_INVALID_CHARS;
    wchar_t* result;

    if (output == NULL) {
        return -1;
    }
    *output = NULL;

    if (input == NULL || input[0] == '\0') {
        result = (wchar_t*)calloc(1, sizeof(wchar_t));
        if (result == NULL) {
            set_error(error, duplicate_string("out of memory"));
            if (error_code != NULL) {
                *error_code = 1;
            }
            return -1;
        }
        *output = result;
        return 0;
    }

    size = MultiByteToWideChar(code_page, flags, input, -1, NULL, 0);
    if (size == 0) {
        /* MSYS2 UCRT64 normally passes UTF-8 argv. Fall back to the active ANSI
           code page only for environments that do not follow that convention. */
        code_page = CP_ACP;
        flags = 0;
        size = MultiByteToWideChar(code_page, flags, input, -1, NULL, 0);
    }

    if (size == 0) {
        DWORD last_error = GetLastError();
        set_error(error, duplicate_string("failed to convert an argument to UTF-16"));
        if (error_code != NULL) {
            *error_code = (uint32_t)last_error;
        }
        return -1;
    }

    result = (wchar_t*)malloc((size_t)size * sizeof(wchar_t));
    if (result == NULL) {
        set_error(error, duplicate_string("out of memory"));
        if (error_code != NULL) {
            *error_code = 1;
        }
        return -1;
    }

    written = MultiByteToWideChar(code_page, flags, input, -1, result, size);
    if (written == 0) {
        DWORD last_error = GetLastError();
        free(result);
        set_error(error, duplicate_string("failed to convert an argument to UTF-16"));
        if (error_code != NULL) {
            *error_code = (uint32_t)last_error;
        }
        return -1;
    }

    *output = result;
    return 0;
}

static char* windows_message(uint32_t code) {
    wchar_t* raw = NULL;
    DWORD flags = FORMAT_MESSAGE_ALLOCATE_BUFFER |
                  FORMAT_MESSAGE_FROM_SYSTEM |
                  FORMAT_MESSAGE_IGNORE_INSERTS;
    DWORD length;
    char* message;
    size_t len;

    length = FormatMessageW(flags, NULL, (DWORD)code, 0,
                            (LPWSTR)&raw, 0, NULL);

    if (length == 0 && HRESULT_FACILITY((HRESULT)code) == FACILITY_WIN32) {
        length = FormatMessageW(flags, NULL, HRESULT_CODE((HRESULT)code), 0,
                                (LPWSTR)&raw, 0, NULL);
    }

    if (length == 0 || raw == NULL) {
        return duplicate_string("");
    }

    while (length > 0 &&
           (raw[length - 1] == L'\r' || raw[length - 1] == L'\n' ||
            raw[length - 1] == L' '  || raw[length - 1] == L'\t')) {
        raw[--length] = L'\0';
    }

    message = wide_to_utf8(raw);
    LocalFree(raw);

    if (message == NULL) {
        return duplicate_string("");
    }

    len = strlen(message);
    while (len > 0 &&
           (message[len - 1] == '\r' || message[len - 1] == '\n' ||
            message[len - 1] == ' '  || message[len - 1] == '\t')) {
        message[--len] = '\0';
    }

    return message;
}

static char* operation_error(const char* operation, uint32_t code) {
    char* sys_message = windows_message(code);
    char* result;

    if (sys_message != NULL && sys_message[0] != '\0') {
        result = format_string("%s failed (code=0x%X); %s",
                               operation, (unsigned int)code, sys_message);
    } else {
        result = format_string("%s failed (code=0x%X)",
                               operation, (unsigned int)code);
    }

    free(sys_message);
    return result;
}

static int full_path_no_expand(const wchar_t* path, wchar_t** output,
                               char** error, uint32_t* error_code) {
    DWORD needed;
    DWORD written;
    wchar_t* buffer;

    if (output == NULL) {
        return -1;
    }
    *output = NULL;

    needed = GetFullPathNameW(path, 0, NULL, NULL);
    if (needed == 0) {
        DWORD last_error = GetLastError();
        set_error(error, operation_error("Normalizing path", (uint32_t)last_error));
        if (error_code != NULL) {
            *error_code = (uint32_t)last_error;
        }
        return -1;
    }

    buffer = (wchar_t*)calloc((size_t)needed, sizeof(wchar_t));
    if (buffer == NULL) {
        set_error(error, duplicate_string("out of memory"));
        if (error_code != NULL) {
            *error_code = 1;
        }
        return -1;
    }

    written = GetFullPathNameW(path, needed, buffer, NULL);
    if (written == 0) {
        DWORD last_error = GetLastError();
        free(buffer);
        set_error(error, operation_error("Normalizing path", (uint32_t)last_error));
        if (error_code != NULL) {
            *error_code = (uint32_t)last_error;
        }
        return -1;
    }

    if (written >= needed) {
        wchar_t* resized;
        needed = written + 1;
        resized = (wchar_t*)realloc(buffer, (size_t)needed * sizeof(wchar_t));
        if (resized == NULL) {
            free(buffer);
            set_error(error, duplicate_string("out of memory"));
            if (error_code != NULL) {
                *error_code = 1;
            }
            return -1;
        }
        buffer = resized;
        memset(buffer, 0, (size_t)needed * sizeof(wchar_t));

        written = GetFullPathNameW(path, needed, buffer, NULL);
        if (written == 0) {
            DWORD last_error = GetLastError();
            free(buffer);
            set_error(error, operation_error("Normalizing path", (uint32_t)last_error));
            if (error_code != NULL) {
                *error_code = (uint32_t)last_error;
            }
            return -1;
        }
    }

    buffer[written] = L'\0';
    *output = buffer;
    return 0;
}

static void wide_path_list_free(WidePathList* list) {
    size_t i;

    if (list == NULL) {
        return;
    }

    for (i = 0; i < list->count; ++i) {
        free(list->items[i]);
    }
    free(list->items);

    list->items = NULL;
    list->count = 0;
    list->capacity = 0;
}

static bool wide_path_list_contains(const WidePathList* list, const wchar_t* path) {
    size_t i;

    if (list == NULL || path == NULL) {
        return false;
    }

    for (i = 0; i < list->count; ++i) {
        if (wcscmp(list->items[i], path) == 0) {
            return true;
        }
    }

    return false;
}

static int wide_path_list_push(WidePathList* list, wchar_t* path,
                               char** error, uint32_t* error_code) {
    wchar_t** resized;
    size_t new_capacity;

    if (list == NULL || path == NULL) {
        return -1;
    }

    if (list->count == list->capacity) {
        new_capacity = (list->capacity == 0) ? 8 : list->capacity * 2;
        resized = (wchar_t**)realloc(list->items, new_capacity * sizeof(wchar_t*));
        if (resized == NULL) {
            set_error(error, duplicate_string("out of memory"));
            if (error_code != NULL) {
                *error_code = 1;
            }
            return -1;
        }
        list->items = resized;
        list->capacity = new_capacity;
    }

    list->items[list->count++] = path;
    return 0;
}

static int normalize_paths_no_expand(char** paths, int path_count,
                                     WidePathList* normalized,
                                     char** error, uint32_t* error_code) {
    int i;

    if (normalized == NULL) {
        return -1;
    }

    normalized->items = NULL;
    normalized->count = 0;
    normalized->capacity = 0;

    for (i = 0; i < path_count; ++i) {
        wchar_t* wide_path = NULL;
        wchar_t* full_path = NULL;

        if (utf8_to_wide(paths[i], &wide_path, error, error_code) != 0) {
            wide_path_list_free(normalized);
            return -1;
        }

        if (full_path_no_expand(wide_path, &full_path, error, error_code) != 0) {
            free(wide_path);
            wide_path_list_free(normalized);
            return -1;
        }

        free(wide_path);

        if (!wide_path_list_contains(normalized, full_path)) {
            if (wide_path_list_push(normalized, full_path, error, error_code) != 0) {
                free(full_path);
                wide_path_list_free(normalized);
                return -1;
            }
        } else {
            free(full_path);
        }
    }

    return 0;
}

static int delete_to_recycle_bin(char** paths, int path_count, FILEOP_FLAGS flags,
                                 FileOpResult* result, char** error, uint32_t* error_code) {
    WidePathList normalized;
    size_t i;
    size_t from_chars = 1;
    size_t pos = 0;
    wchar_t* from;
    SHFILEOPSTRUCTW op;

    if (result == NULL) {
        return -1;
    }

    result->retcode = 0;
    result->aborted = false;

    if (normalize_paths_no_expand(paths, path_count, &normalized, error, error_code) != 0) {
        return -1;
    }

    if (normalized.count == 0) {
        wide_path_list_free(&normalized);
        return 0;
    }

    for (i = 0; i < normalized.count; ++i) {
        from_chars += wcslen(normalized.items[i]) + 1;
    }

    from = (wchar_t*)calloc(from_chars, sizeof(wchar_t));
    if (from == NULL) {
        wide_path_list_free(&normalized);
        set_error(error, duplicate_string("out of memory"));
        if (error_code != NULL) {
            *error_code = 1;
        }
        return -1;
    }

    for (i = 0; i < normalized.count; ++i) {
        size_t len = wcslen(normalized.items[i]);
        memcpy(from + pos, normalized.items[i], len * sizeof(wchar_t));
        pos += len;
        from[pos++] = L'\0';
    }
    from[pos++] = L'\0';

    memset(&op, 0, sizeof(op));
    op.hwnd = NULL;
    op.wFunc = FO_DELETE;
    op.pFrom = from;
    op.pTo = NULL;
    op.fFlags = flags;
    op.fAnyOperationsAborted = FALSE;
    op.hNameMappings = NULL;
    op.lpszProgressTitle = NULL;

    result->retcode = SHFileOperationW(&op);
    result->aborted = (op.fAnyOperationsAborted != FALSE);

    free(from);
    wide_path_list_free(&normalized);
    return 0;
}

static int send_to_recycle_bin(char** paths, int path_count,
                               bool allow_undo,
                               bool suppress_confirmation,
                               bool* completed,
                               char** error, uint32_t* error_code) {
    FILEOP_FLAGS flags = 0;
    FileOpResult op_result;

    if (completed != NULL) {
        *completed = false;
    }

    if (allow_undo) {
        flags |= FOF_ALLOWUNDO;
    }
    if (suppress_confirmation) {
        flags |= FOF_NOCONFIRMATION;
    }

    if (delete_to_recycle_bin(paths, path_count, flags, &op_result, error, error_code) != 0) {
        return -1;
    }

    if (op_result.retcode != 0) {
        set_error(error, operation_error("Moving files to the Recycle Bin",
                                         (uint32_t)op_result.retcode));
        if (error_code != NULL) {
            *error_code = (uint32_t)op_result.retcode;
        }
        return -1;
    }

    if (completed != NULL) {
        *completed = !op_result.aborted;
    }
    return 0;
}

static int get_recycle_bin_status(RecycleBinStatus* status,
                                  char** error, uint32_t* error_code) {
    SHQUERYRBINFO info;
    HRESULT hr;

    if (status == NULL) {
        return -1;
    }

    memset(&info, 0, sizeof(info));
    info.cbSize = sizeof(info);

    hr = SHQueryRecycleBinW(NULL, &info);
    if (FAILED(hr)) {
        set_error(error, operation_error("Querying the Recycle Bin", (uint32_t)hr));
        if (error_code != NULL) {
            *error_code = (uint32_t)hr;
        }
        return -1;
    }

    status->size_bytes = (unsigned long long)info.i64Size;
    status->items = (unsigned long long)info.i64NumItems;
    return 0;
}

static int empty_recycle_bin_with_options(bool require_confirmation,
                                          bool show_progress,
                                          bool play_sound,
                                          char** error, uint32_t* error_code) {
    DWORD flags = 0;
    HRESULT hr;

    if (!require_confirmation) {
        flags |= SHERB_NOCONFIRMATION;
    }
    if (!show_progress) {
        flags |= SHERB_NOPROGRESSUI;
    }
    if (!play_sound) {
        flags |= SHERB_NOSOUND;
    }

    hr = SHEmptyRecycleBinW(NULL, NULL, flags);
    if (FAILED(hr)) {
        set_error(error, operation_error("Emptying the Recycle Bin", (uint32_t)hr));
        if (error_code != NULL) {
            *error_code = (uint32_t)hr;
        }
        return -1;
    }

    return 0;
}

static void human_size(unsigned long long bytes, char* output, size_t output_size) {
    static const char* units[] = {"B", "KB", "MB", "GB", "TB", "PB"};
    const size_t unit_count = sizeof(units) / sizeof(units[0]);
    double value = (double)bytes;
    size_t unit_index = 0;

    while (value >= 1024.0 && unit_index + 1 < unit_count) {
        value /= 1024.0;
        ++unit_index;
    }

    snprintf(output, output_size, "%.1f%s", value, units[unit_index]);
}

static void print_usage(const char* program_name) {
    printf("Usage:\n");
    printf("  %s [PATH ...]\n", program_name);
    printf("  %s --status\n", program_name);
    printf("  %s --empty [--confirm-empty] [--no-sound]\n", program_name);
    printf("\n");
    printf("Options:\n");
    printf("  --status          Show Recycle Bin total size and item count.\n");
    printf("  --empty           Empty the Recycle Bin.\n");
    printf("  --confirm-empty   Prompt in the terminal before emptying.\n");
    printf("  --no-sound        Suppress the Recycle Bin empty sound.\n");
    printf("  -v, --verbose     Print additional status messages. Can be repeated.\n");
    printf("  -h, --help        Show this help message.\n");
    printf("\n");
    printf("PATH values are made absolute and normalized, but environment variables, '~',\n");
    printf("and wildcards are not expanded by rmtrash.\n");
}

static int parse_options(int argc, char* argv[], Options* options, char** error) {
    static struct option long_options[] = {
        {"status",        no_argument, NULL, OPT_STATUS},
        {"empty",         no_argument, NULL, OPT_EMPTY},
        {"no-sound",      no_argument, NULL, OPT_NO_SOUND},
        {"confirm-empty", no_argument, NULL, OPT_CONFIRM_EMPTY},
        {"verbose",       no_argument, NULL, 'v'},
        {"help",          no_argument, NULL, 'h'},
        {NULL, 0, NULL, 0}
    };

    if (options == NULL) {
        return -1;
    }

    memset(options, 0, sizeof(*options));

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
            options->status = true;
            break;
        case OPT_EMPTY:
            options->empty = true;
            break;
        case OPT_NO_SOUND:
            options->no_sound = true;
            break;
        case OPT_CONFIRM_EMPTY:
            options->confirm_empty = true;
            break;
        case 'v':
            ++options->verbose;
            break;
        case 'h':
            options->show_help = true;
            break;
        case '?': {
            const char* opt = (optind > 0 && optind <= argc) ? argv[optind - 1] : "";
            if (opt[0] == '\0') {
                set_error(error, duplicate_string("unknown option"));
            } else {
                set_error(error, format_string("unknown option: %s", opt));
            }
            return -1;
        }
        default:
            set_error(error, duplicate_string("failed to parse command-line options"));
            return -1;
        }
    }

    options->paths = argv + optind;
    options->path_count = argc - optind;
    return 0;
}

static bool confirm_empty(void) {
    char response[256];
    char* start;
    char* end;
    char* p;

    printf("Empty the Recycle Bin now? This cannot be undone. [y/N]: ");
    fflush(stdout);

    if (fgets(response, sizeof(response), stdin) == NULL) {
        return false;
    }

    start = response;
    while (*start != '\0' && isspace((unsigned char)*start)) {
        ++start;
    }

    end = start + strlen(start);
    while (end > start && isspace((unsigned char)*(end - 1))) {
        --end;
    }
    *end = '\0';

    for (p = start; *p != '\0'; ++p) {
        *p = (char)tolower((unsigned char)*p);
    }

    return strcmp(start, "y") == 0 || strcmp(start, "yes") == 0;
}

int main(int argc, char* argv[]) {
    Options options;
    char* error = NULL;
    uint32_t error_code = 0;

    SetConsoleOutputCP(CP_UTF8);

    if (parse_options(argc, argv, &options, &error) != 0) {
        fprintf(stderr, "%s\n\n", error != NULL ? error : "failed to parse command-line options");
        free(error);
        print_usage(argv[0]);
        return 2;
    }

    if (options.show_help) {
        print_usage(argv[0]);
        return 0;
    }

    if (!options.status && !options.empty && options.path_count == 0) {
        print_usage(argv[0]);
        return 0;
    }

    if (options.status) {
        RecycleBinStatus status;
        char readable_size[64];

        if (get_recycle_bin_status(&status, &error, &error_code) != 0) {
            (void)error_code;
            fprintf(stderr, "%s\n", error != NULL ? error : "Querying the Recycle Bin failed");
            free(error);
            return 1;
        }

        human_size(status.size_bytes, readable_size, sizeof(readable_size));
        printf("Recycle Bin status:\n");
        printf("  items : %llu\n", status.items);
        printf("  size  : %llu bytes (%s)\n", status.size_bytes, readable_size);
    }

    if (options.empty) {
        if (options.confirm_empty && !confirm_empty()) {
            printf("Aborted by user.\n");
            return 0;
        }

        if (empty_recycle_bin_with_options(options.confirm_empty, false, !options.no_sound,
                                           &error, &error_code) != 0) {
            (void)error_code;
            fprintf(stderr, "%s\n", error != NULL ? error : "Emptying the Recycle Bin failed");
            free(error);
            return 1;
        }

        printf("Recycle Bin emptied.\n");
    }

    if (options.path_count > 0) {
        bool completed = false;
        int i;

        if (send_to_recycle_bin(options.paths, options.path_count, true, true,
                                &completed, &error, &error_code) != 0) {
            (void)error_code;
            fprintf(stderr, "%s\n", error != NULL ? error : "Moving files to the Recycle Bin failed");
            free(error);
            return 1;
        }

        if (options.verbose > 0) {
            if (completed) {
                printf("Moved to recycle bin:");
                for (i = 0; i < options.path_count; ++i) {
                    printf(" %s", options.paths[i]);
                }
                printf("\n");
            } else {
                printf("Recycle Bin operation was aborted by the shell.\n");
            }
        }
    }

    free(error);
    return 0;
}
