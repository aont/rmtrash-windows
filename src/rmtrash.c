#ifndef WIN32_LEAN_AND_MEAN
#define WIN32_LEAN_AND_MEAN
#endif
#include <windows.h>
#include <shellapi.h>
#include <shlobj.h>

#define OPT_STATUS        1000
#define OPT_EMPTY         1001
#define OPT_NO_SOUND      1002
#define OPT_CONFIRM_EMPTY 1003

#define EXIT_OK 0
#define EXIT_ERROR 1
#define EXIT_USAGE 2

typedef int bool_t;
#define TRUE_T 1
#define FALSE_T 0

typedef struct RecycleBinStatus {
    unsigned long long size_bytes;
    unsigned long long items;
} RecycleBinStatus;

typedef struct Options {
    bool_t show_help;
    bool_t status;
    bool_t empty;
    bool_t no_sound;
    bool_t confirm_empty;
    int verbose;
    wchar_t** paths;
    int path_count;
} Options;

typedef struct WidePathList {
    wchar_t** items;
    SIZE_T count;
    SIZE_T capacity;
} WidePathList;

typedef struct FileOpResult {
    int retcode;
    bool_t aborted;
} FileOpResult;

static SIZE_T string_length(const wchar_t* s) {
    SIZE_T len = 0;
    if (s == NULL) {
        return 0;
    }
    while (s[len] != L'\0') {
        ++len;
    }
    return len;
}

static int string_compare(const wchar_t* a, const wchar_t* b) {
    SIZE_T i = 0;
    if (a == NULL) {
        a = L"";
    }
    if (b == NULL) {
        b = L"";
    }
    while (a[i] != L'\0' && a[i] == b[i]) {
        ++i;
    }
    return (int)a[i] - (int)b[i];
}

static int string_equal(const wchar_t* a, const wchar_t* b) {
    return string_compare(a, b) == 0;
}

static void* memory_set(void* dest, int value, SIZE_T size) {
    unsigned char* p = (unsigned char*)dest;
    while (size-- > 0) {
        *p++ = (unsigned char)value;
    }
    return dest;
}

static void* memory_copy(void* dest, const void* src, SIZE_T size) {
    unsigned char* d = (unsigned char*)dest;
    const unsigned char* s = (const unsigned char*)src;
    while (size-- > 0) {
        *d++ = *s++;
    }
    return dest;
}

void* memset(void* dest, int value, SIZE_T size) {
    return memory_set(dest, value, size);
}

void* memcpy(void* dest, const void* src, SIZE_T size) {
    return memory_copy(dest, src, size);
}

static HANDLE process_heap(void) {
    return GetProcessHeap();
}

static void* alloc_bytes(SIZE_T size) {
    if (size == 0) {
        size = 1;
    }
    return HeapAlloc(process_heap(), 0, size);
}

static void* alloc_zero(SIZE_T count, SIZE_T size) {
    if (count != 0 && size > ((SIZE_T)-1) / count) {
        return NULL;
    }
    return HeapAlloc(process_heap(), HEAP_ZERO_MEMORY, count * size);
}

static void* resize_bytes(void* ptr, SIZE_T size) {
    if (ptr == NULL) {
        return alloc_bytes(size);
    }
    if (size == 0) {
        size = 1;
    }
    return HeapReAlloc(process_heap(), 0, ptr, size);
}

static void free_mem(void* ptr) {
    if (ptr != NULL) {
        HeapFree(process_heap(), 0, ptr);
    }
}

static wchar_t* duplicate_string(const wchar_t* s) {
    SIZE_T len;
    wchar_t* copy;
    if (s == NULL) {
        s = L"";
    }
    len = string_length(s);
    copy = (wchar_t*)alloc_bytes((len + 1) * sizeof(wchar_t));
    if (copy == NULL) {
        return NULL;
    }
    memory_copy(copy, s, (len + 1) * sizeof(wchar_t));
    return copy;
}

static void set_error(wchar_t** error, wchar_t* message) {
    if (error == NULL) {
        free_mem(message);
        return;
    }
    free_mem(*error);
    *error = message;
}

static void append_char(wchar_t* buffer, SIZE_T buffer_size, SIZE_T* pos, wchar_t ch) {
    if (*pos + 1 < buffer_size) {
        buffer[*pos] = ch;
        ++(*pos);
        buffer[*pos] = L'\0';
    }
}

static void append_string(wchar_t* buffer, SIZE_T buffer_size, SIZE_T* pos, const wchar_t* text) {
    SIZE_T i;
    if (text == NULL) {
        return;
    }
    for (i = 0; text[i] != L'\0'; ++i) {
        append_char(buffer, buffer_size, pos, text[i]);
    }
}

static void append_uint(wchar_t* buffer, SIZE_T buffer_size, SIZE_T* pos, unsigned long long value) {
    wchar_t digits[32];
    SIZE_T count = 0;
    if (value == 0) {
        append_char(buffer, buffer_size, pos, L'0');
        return;
    }
    while (value > 0 && count < 32) {
        digits[count++] = (wchar_t)(L'0' + (value % 10));
        value /= 10;
    }
    while (count > 0) {
        append_char(buffer, buffer_size, pos, digits[--count]);
    }
}

static void append_hex(wchar_t* buffer, SIZE_T buffer_size, SIZE_T* pos, unsigned int value) {
    static const wchar_t hex[] = L"0123456789ABCDEF";
    int started = 0;
    int shift;
    append_string(buffer, buffer_size, pos, L"0x");
    for (shift = 28; shift >= 0; shift -= 4) {
        unsigned int nibble = (value >> shift) & 0xFU;
        if (nibble != 0 || started || shift == 0) {
            started = 1;
            append_char(buffer, buffer_size, pos, hex[nibble]);
        }
    }
}

static wchar_t* make_operation_error(const wchar_t* operation, unsigned int code) {
    wchar_t* raw = NULL;
    DWORD flags = FORMAT_MESSAGE_ALLOCATE_BUFFER | FORMAT_MESSAGE_FROM_SYSTEM | FORMAT_MESSAGE_IGNORE_INSERTS;
    DWORD length;
    wchar_t* result;
    SIZE_T pos = 0;

    length = FormatMessageW(flags, NULL, (DWORD)code, 0, (LPWSTR)&raw, 0, NULL);
    if (length == 0 && HRESULT_FACILITY((HRESULT)code) == FACILITY_WIN32) {
        length = FormatMessageW(flags, NULL, HRESULT_CODE((HRESULT)code), 0, (LPWSTR)&raw, 0, NULL);
    }
    while (length > 0 && (raw[length - 1] == L'\r' || raw[length - 1] == L'\n' || raw[length - 1] == L' ' || raw[length - 1] == L'\t')) {
        raw[--length] = L'\0';
    }

    result = (wchar_t*)alloc_zero(1024, sizeof(wchar_t));
    if (result == NULL) {
        if (raw != NULL) {
            LocalFree(raw);
        }
        return NULL;
    }
    append_string(result, 1024, &pos, operation);
    append_string(result, 1024, &pos, L" failed (code=");
    append_hex(result, 1024, &pos, code);
    append_char(result, 1024, &pos, L')');
    if (raw != NULL && raw[0] != L'\0') {
        append_string(result, 1024, &pos, L"; ");
        append_string(result, 1024, &pos, raw);
    }
    if (raw != NULL) {
        LocalFree(raw);
    }
    return result;
}

static void write_handle(HANDLE handle, const wchar_t* text) {
    DWORD written;
    if (text != NULL) {
        WriteConsoleW(handle, text, (DWORD)string_length(text), &written, NULL);
    }
}

static void print_out(const wchar_t* text) { write_handle(GetStdHandle(STD_OUTPUT_HANDLE), text); }
static void print_err(const wchar_t* text) { write_handle(GetStdHandle(STD_ERROR_HANDLE), text); }

static void print_usage(const wchar_t* program_name) {
    if (program_name == NULL || program_name[0] == L'\0') {
        program_name = L"rmtrash";
    }
    print_out(L"Usage:\n  "); print_out(program_name); print_out(L" [PATH ...]\n  ");
    print_out(program_name); print_out(L" --status\n  ");
    print_out(program_name); print_out(L" --empty [--confirm-empty] [--no-sound]\n\n");
    print_out(L"Options:\n  --status          Show Recycle Bin total size and item count.\n");
    print_out(L"  --empty           Empty the Recycle Bin.\n  --confirm-empty   Prompt in the terminal before emptying.\n");
    print_out(L"  --no-sound        Suppress the Recycle Bin empty sound.\n  -v, --verbose     Print additional status messages. Can be repeated.\n");
    print_out(L"  -h, --help        Show this help message.\n\n");
    print_out(L"PATH values are made absolute and normalized, but environment variables, '~',\nand wildcards are not expanded by rmtrash.\n");
}

static int full_path_no_expand(const wchar_t* path, wchar_t** output, wchar_t** error, unsigned int* error_code) {
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
        set_error(error, make_operation_error(L"Normalizing path", (unsigned int)last_error));
        if (error_code != NULL) *error_code = (unsigned int)last_error;
        return -1;
    }
    buffer = (wchar_t*)alloc_zero(needed, sizeof(wchar_t));
    if (buffer == NULL) {
        set_error(error, duplicate_string(L"out of memory"));
        if (error_code != NULL) *error_code = 1;
        return -1;
    }
    written = GetFullPathNameW(path, needed, buffer, NULL);
    if (written == 0) {
        DWORD last_error = GetLastError();
        free_mem(buffer);
        set_error(error, make_operation_error(L"Normalizing path", (unsigned int)last_error));
        if (error_code != NULL) *error_code = (unsigned int)last_error;
        return -1;
    }
    buffer[written] = L'\0';
    *output = buffer;
    return 0;
}

static void wide_path_list_free(WidePathList* list) {
    SIZE_T i;
    if (list == NULL) return;
    for (i = 0; i < list->count; ++i) free_mem(list->items[i]);
    free_mem(list->items);
    list->items = NULL; list->count = 0; list->capacity = 0;
}

static int wide_path_list_contains(const WidePathList* list, const wchar_t* path) {
    SIZE_T i;
    if (list == NULL || path == NULL) return FALSE_T;
    for (i = 0; i < list->count; ++i) {
        if (string_equal(list->items[i], path)) return TRUE_T;
    }
    return FALSE_T;
}

static int wide_path_list_push(WidePathList* list, wchar_t* path, wchar_t** error, unsigned int* error_code) {
    wchar_t** resized;
    SIZE_T new_capacity;
    if (list == NULL || path == NULL) return -1;
    if (list->count == list->capacity) {
        new_capacity = (list->capacity == 0) ? 8 : list->capacity * 2;
        resized = (wchar_t**)resize_bytes(list->items, new_capacity * sizeof(wchar_t*));
        if (resized == NULL) {
            set_error(error, duplicate_string(L"out of memory"));
            if (error_code != NULL) *error_code = 1;
            return -1;
        }
        list->items = resized;
        list->capacity = new_capacity;
    }
    list->items[list->count++] = path;
    return 0;
}

static int normalize_paths_no_expand(wchar_t** paths, int path_count, WidePathList* normalized, wchar_t** error, unsigned int* error_code) {
    int i;
    if (normalized == NULL) return -1;
    normalized->items = NULL; normalized->count = 0; normalized->capacity = 0;
    for (i = 0; i < path_count; ++i) {
        wchar_t* full_path = NULL;
        if (full_path_no_expand(paths[i], &full_path, error, error_code) != 0) {
            wide_path_list_free(normalized);
            return -1;
        }
        if (!wide_path_list_contains(normalized, full_path)) {
            if (wide_path_list_push(normalized, full_path, error, error_code) != 0) {
                free_mem(full_path);
                wide_path_list_free(normalized);
                return -1;
            }
        } else {
            free_mem(full_path);
        }
    }
    return 0;
}

static int delete_to_recycle_bin(wchar_t** paths, int path_count, FILEOP_FLAGS flags, FileOpResult* result, wchar_t** error, unsigned int* error_code) {
    WidePathList normalized;
    SIZE_T i;
    SIZE_T from_chars = 1;
    SIZE_T pos = 0;
    wchar_t* from;
    SHFILEOPSTRUCTW op;
    if (result == NULL) return -1;
    result->retcode = 0; result->aborted = FALSE_T;
    if (normalize_paths_no_expand(paths, path_count, &normalized, error, error_code) != 0) return -1;
    if (normalized.count == 0) { wide_path_list_free(&normalized); return 0; }
    for (i = 0; i < normalized.count; ++i) from_chars += string_length(normalized.items[i]) + 1;
    from = (wchar_t*)alloc_zero(from_chars, sizeof(wchar_t));
    if (from == NULL) {
        wide_path_list_free(&normalized);
        set_error(error, duplicate_string(L"out of memory"));
        if (error_code != NULL) *error_code = 1;
        return -1;
    }
    for (i = 0; i < normalized.count; ++i) {
        SIZE_T len = string_length(normalized.items[i]);
        memory_copy(from + pos, normalized.items[i], len * sizeof(wchar_t));
        pos += len;
        from[pos++] = L'\0';
    }
    from[pos++] = L'\0';
    memory_set(&op, 0, sizeof(op));
    op.wFunc = FO_DELETE;
    op.pFrom = from;
    op.fFlags = flags;
    result->retcode = SHFileOperationW(&op);
    result->aborted = (op.fAnyOperationsAborted != FALSE);
    free_mem(from);
    wide_path_list_free(&normalized);
    return 0;
}

static int send_to_recycle_bin(wchar_t** paths, int path_count, bool_t* completed, wchar_t** error, unsigned int* error_code) {
    FileOpResult op_result;
    if (completed != NULL) *completed = FALSE_T;
    if (delete_to_recycle_bin(paths, path_count, FOF_ALLOWUNDO | FOF_NOCONFIRMATION, &op_result, error, error_code) != 0) return -1;
    if (op_result.retcode != 0) {
        set_error(error, make_operation_error(L"Moving files to the Recycle Bin", (unsigned int)op_result.retcode));
        if (error_code != NULL) *error_code = (unsigned int)op_result.retcode;
        return -1;
    }
    if (completed != NULL) *completed = !op_result.aborted;
    return 0;
}

static int get_recycle_bin_status(RecycleBinStatus* status, wchar_t** error, unsigned int* error_code) {
    SHQUERYRBINFO info;
    HRESULT hr;
    if (status == NULL) return -1;
    memory_set(&info, 0, sizeof(info));
    info.cbSize = sizeof(info);
    hr = SHQueryRecycleBinW(NULL, &info);
    if (FAILED(hr)) {
        set_error(error, make_operation_error(L"Querying the Recycle Bin", (unsigned int)hr));
        if (error_code != NULL) *error_code = (unsigned int)hr;
        return -1;
    }
    status->size_bytes = (unsigned long long)info.i64Size;
    status->items = (unsigned long long)info.i64NumItems;
    return 0;
}

static int empty_recycle_bin_with_options(bool_t require_confirmation, bool_t play_sound, wchar_t** error, unsigned int* error_code) {
    DWORD flags = SHERB_NOPROGRESSUI;
    HRESULT hr;
    if (!require_confirmation) flags |= SHERB_NOCONFIRMATION;
    if (!play_sound) flags |= SHERB_NOSOUND;
    hr = SHEmptyRecycleBinW(NULL, NULL, flags);
    if (FAILED(hr)) {
        set_error(error, make_operation_error(L"Emptying the Recycle Bin", (unsigned int)hr));
        if (error_code != NULL) *error_code = (unsigned int)hr;
        return -1;
    }
    return 0;
}

static void append_human_size(wchar_t* buffer, SIZE_T buffer_size, SIZE_T* pos, unsigned long long bytes) {
    static const wchar_t* units[] = {L"B", L"KB", L"MB", L"GB", L"TB", L"PB"};
    unsigned long long scaled = bytes * 10;
    int unit_index = 0;
    while (scaled >= 10240 && unit_index < 5) {
        scaled = (scaled + 512) / 1024;
        ++unit_index;
    }
    append_uint(buffer, buffer_size, pos, scaled / 10);
    append_char(buffer, buffer_size, pos, L'.');
    append_uint(buffer, buffer_size, pos, scaled % 10);
    append_string(buffer, buffer_size, pos, units[unit_index]);
}

static int starts_with_dash(const wchar_t* arg) {
    return arg != NULL && arg[0] == L'-' && arg[1] != L'\0';
}

static int parse_options(int argc, wchar_t** argv, Options* options, wchar_t** error) {
    int i;
    if (options == NULL) return -1;
    memory_set(options, 0, sizeof(*options));
    for (i = 1; i < argc; ++i) {
        wchar_t* arg = argv[i];
        if (string_equal(arg, L"--")) {
            ++i;
            break;
        }
        if (!starts_with_dash(arg)) {
            break;
        }
        if (string_equal(arg, L"--status")) options->status = TRUE_T;
        else if (string_equal(arg, L"--empty")) options->empty = TRUE_T;
        else if (string_equal(arg, L"--no-sound")) options->no_sound = TRUE_T;
        else if (string_equal(arg, L"--confirm-empty")) options->confirm_empty = TRUE_T;
        else if (string_equal(arg, L"--verbose") || string_equal(arg, L"-v")) ++options->verbose;
        else if (string_equal(arg, L"--help") || string_equal(arg, L"-h")) options->show_help = TRUE_T;
        else {
            set_error(error, duplicate_string(L"unknown option"));
            return -1;
        }
    }
    options->paths = argv + i;
    options->path_count = argc - i;
    return 0;
}

static bool_t confirm_empty(void) {
    wchar_t response[16];
    DWORD read = 0;
    DWORD i;
    print_out(L"Empty the Recycle Bin now? This cannot be undone. [y/N]: ");
    if (!ReadConsoleW(GetStdHandle(STD_INPUT_HANDLE), response, 15, &read, NULL) || read == 0) {
        return FALSE_T;
    }
    response[read < 15 ? read : 15] = L'\0';
    for (i = 0; i < read; ++i) {
        if (response[i] >= L'A' && response[i] <= L'Z') response[i] = (wchar_t)(response[i] + (L'a' - L'A'));
        if (response[i] == L'\r' || response[i] == L'\n' || response[i] == L' ' || response[i] == L'\t') response[i] = L'\0';
    }
    return string_equal(response, L"y") || string_equal(response, L"yes");
}

static int run_program(void) {
    int argc = 0;
    wchar_t** argv = CommandLineToArgvW(GetCommandLineW(), &argc);
    Options options;
    wchar_t* error = NULL;
    unsigned int error_code = 0;
    int exit_code = EXIT_OK;

    SetConsoleOutputCP(CP_UTF8);
    if (argv == NULL) {
        print_err(L"failed to read command line\n");
        return EXIT_ERROR;
    }

    if (parse_options(argc, argv, &options, &error) != 0) {
        print_err(error != NULL ? error : L"failed to parse command-line options");
        print_err(L"\n\n");
        free_mem(error);
        print_usage(argc > 0 ? argv[0] : L"rmtrash");
        LocalFree(argv);
        return EXIT_USAGE;
    }

    if (options.show_help) {
        print_usage(argc > 0 ? argv[0] : L"rmtrash");
        LocalFree(argv);
        return EXIT_OK;
    }

    if (!options.status && !options.empty && options.path_count == 0) {
        print_usage(argc > 0 ? argv[0] : L"rmtrash");
        LocalFree(argv);
        return EXIT_OK;
    }

    if (options.status) {
        RecycleBinStatus status;
        wchar_t line[256];
        SIZE_T pos = 0;
        if (get_recycle_bin_status(&status, &error, &error_code) != 0) {
            (void)error_code;
            print_err(error != NULL ? error : L"Querying the Recycle Bin failed"); print_err(L"\n");
            free_mem(error); LocalFree(argv); return EXIT_ERROR;
        }
        print_out(L"Recycle Bin status:\n  items : ");
        pos = 0; append_uint(line, 256, &pos, status.items); append_char(line, 256, &pos, L'\0'); print_out(line);
        print_out(L"\n  size  : ");
        pos = 0; append_uint(line, 256, &pos, status.size_bytes); append_string(line, 256, &pos, L" bytes ("); append_human_size(line, 256, &pos, status.size_bytes); append_string(line, 256, &pos, L")\n"); print_out(line);
    }

    if (options.empty) {
        if (options.confirm_empty && !confirm_empty()) {
            print_out(L"Aborted by user.\n");
            LocalFree(argv); return EXIT_OK;
        }
        if (empty_recycle_bin_with_options(options.confirm_empty, !options.no_sound, &error, &error_code) != 0) {
            (void)error_code;
            print_err(error != NULL ? error : L"Emptying the Recycle Bin failed"); print_err(L"\n");
            free_mem(error); LocalFree(argv); return EXIT_ERROR;
        }
        print_out(L"Recycle Bin emptied.\n");
    }

    if (options.path_count > 0) {
        bool_t completed = FALSE_T;
        int i;
        if (send_to_recycle_bin(options.paths, options.path_count, &completed, &error, &error_code) != 0) {
            (void)error_code;
            print_err(error != NULL ? error : L"Moving files to the Recycle Bin failed"); print_err(L"\n");
            free_mem(error); LocalFree(argv); return EXIT_ERROR;
        }
        if (options.verbose > 0) {
            if (completed) {
                print_out(L"Moved to recycle bin:");
                for (i = 0; i < options.path_count; ++i) { print_out(L" "); print_out(options.paths[i]); }
                print_out(L"\n");
            } else {
                print_out(L"Recycle Bin operation was aborted by the shell.\n");
            }
        }
    }

    free_mem(error);
    LocalFree(argv);
    return exit_code;
}

void mainCRTStartup(void) {
    ExitProcess((UINT)run_program());
}
