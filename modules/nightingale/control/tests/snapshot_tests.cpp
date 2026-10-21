#include "../bridge.cpp"
#include <fstream>
#include <iostream>
#include <iterator>
#include <string>
#include <vector>

namespace {
unsigned checks{};
void check(bool condition) { ++checks; if (!condition) throw std::runtime_error("snapshot check " + std::to_string(checks)); }
std::vector<uint8_t> read(const std::wstring& path) {
    std::ifstream file(path, std::ios::binary);
    if (!file) throw std::runtime_error("fixture missing");
    return {std::istreambuf_iterator<char>(file), std::istreambuf_iterator<char>()};
}
}
int wmain(int argc, wchar_t** argv) {
    try {
        check(argc == 3);
        // Exercise the real public zlib C ABI already installed with Git.
        // Production uses the game's fixed zlib 1.3 RVAs, never this DLL.
        const auto library = LoadLibraryExW(argv[1], nullptr, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS);
        check(library != nullptr);
        nightingale_save::Gzip gzip{
            reinterpret_cast<decltype(nightingale_save::Gzip::initialize)>(GetProcAddress(library, "inflateInit2_")),
            reinterpret_cast<decltype(nightingale_save::Gzip::inflate)>(GetProcAddress(library, "inflate")),
            reinterpret_cast<decltype(nightingale_save::Gzip::finish)>(GetProcAddress(library, "inflateEnd")), "1.3"};
        const auto payload = read(std::wstring(argv[2]) + L"/expected.bin");
        const auto packed = read(std::wstring(argv[2]) + L"/valid.gz");
        const auto expected = nightingale_save::Content::capture(payload);
        using Issue = nightingale_save::SnapshotIssue;
        check(gzip.verify(packed, expected) == Issue::none);
        auto bad_packet = packed; bad_packet[0] ^= 1; check(gzip.verify(bad_packet, expected) == Issue::gzip_shape);
        bad_packet = packed; bad_packet[bad_packet.size() - 8] ^= 1; check(gzip.verify(bad_packet, expected) == Issue::inflate);
        bad_packet = packed; bad_packet.push_back(0); check(gzip.verify(bad_packet, expected) == Issue::trailing_bytes);
        auto altered = expected; altered.hash[0] ^= 1; check(gzip.verify(packed, altered) == Issue::hash);
        altered = expected; ++altered.length; check(gzip.verify(packed, altered) == Issue::length);
        check(gzip.verify({}, expected) == Issue::unavailable);
        check(expected.length == payload.size() && gzip.matches(packed, expected));
        auto copy = packed; copy.pop_back(); check(!gzip.matches(copy, expected));
        copy = packed; copy[copy.size() - 8] ^= 1; check(!gzip.matches(copy, expected));
        copy = packed; copy.back() ^= 1; check(!gzip.matches(copy, expected));
        copy = packed; copy.push_back(0); check(!gzip.matches(copy, expected));
        copy = packed; copy.insert(copy.end(), packed.begin(), packed.end()); check(!gzip.matches(copy, expected));
        auto different = expected; different.hash[0] ^= 1; check(!gzip.matches(packed, different));
        different = expected; --different.length; check(!gzip.matches(packed, different));
        different = expected; ++different.length; check(!gzip.matches(packed, different));
        different = expected; different.length = 0; check(!gzip.matches(packed, different));
        different = expected; different.length = nightingale_save::MAX_BYTES + 1; check(!gzip.matches(packed, different));
        check(!gzip.matches({}, expected));
        check(!gzip.matches(std::span(packed.data(), 17), expected));
        copy = packed; copy[0] ^= 1; check(!gzip.matches(copy, expected));
        copy = packed; copy[2] = 0; check(!gzip.matches(copy, expected));
        auto unavailable = gzip; unavailable.initialize = nullptr; check(!unavailable.matches(packed, expected));
        unavailable = gzip; unavailable.inflate = nullptr; check(!unavailable.matches(packed, expected));
        unavailable = gzip; unavailable.finish = nullptr; check(!unavailable.matches(packed, expected));
        unavailable = gzip; unavailable.version = nullptr; check(!unavailable.matches(packed, expected));
        bool rejected = false;
        try { nightingale_save::Content::capture({}); } catch (const std::runtime_error&) { rejected = true; }
        check(rejected);
        check(leaf(L"DedicatedServer") && leaf(L"b-server--mtr-frt-0000"));
        check(!leaf(L"..") && !leaf(L"../foreign") && !leaf(L"D:\\outside") && !leaf(L"bad:name") && !leaf(L""));
        check(!leaf(std::wstring(193, L'a')));
        NativeString empty_namespace{};
        check(text_at(reinterpret_cast<uintptr_t>(&empty_namespace), 96, "native_backend_namespace", true).empty());
        bool empty_rejected = false;
        try { text_at(reinterpret_cast<uintptr_t>(&empty_namespace), 192, "native_record"); }
        catch (const std::runtime_error&) { empty_rejected = true; }
        check(empty_rejected);
        std::wstring terminator;
        NativeString nul_namespace{terminator.data(), 1, 1};
        check(text_at(reinterpret_cast<uintptr_t>(&nul_namespace), 96, "native_module_namespace", true).empty());
        for (NativeString invalid : {NativeString{nullptr, -1, 0}, NativeString{nullptr, 2, 1}, NativeString{nullptr, 2, 2}}) {
            bool invalid_rejected = false;
            try { text_at(reinterpret_cast<uintptr_t>(&invalid), 96, "native_backend_namespace", true); }
            catch (const std::runtime_error&) { invalid_rejected = true; }
            check(invalid_rejected);
        }
        check((fs::path(L"C:/owned/NWX/Saved") / L"Offline" / L"" / L"Deploy" / L"record" / L"state").lexically_normal()
            == fs::path(L"C:/owned/NWX/Saved/Offline/Deploy/record/state").lexically_normal());
        const fs::path runtime = L"C:\\owned\\runtime";
        const auto native_saved = (runtime / L"NWX/Binaries/Win64/../../../NWX/Saved/").lexically_normal().parent_path();
        check(!equal_path(native_saved, runtime / L"NWX/Saved"));
        check(equal_path(native_saved, (runtime / L"NWX/Saved").lexically_normal()));
        check(!equal_path(native_saved, (runtime.parent_path() / L"NWX/Saved").lexically_normal()));
        Storage identity;
        identity.provider = L"fixture_provider"; identity.principal = L"fixture_principal";
        identity.record = L"fixture_record"; identity.saved = native_saved;
        identity.primary = native_saved / L"Offline/fixture_provider/Deploy/fixture_record/state";
        check(same_storage(identity, identity));
        auto changed_identity = identity; changed_identity.provider = L"other_provider";
        check(!same_storage(identity, changed_identity));
        changed_identity = identity; changed_identity.principal = L"other_principal";
        check(!same_storage(identity, changed_identity));
        changed_identity = identity; changed_identity.client_token = true;
        check(!same_storage(identity, changed_identity));
        changed_identity = identity; changed_identity.name_space = L"other_configuration";
        check(!same_storage(identity, changed_identity));
        const fs::path primary = fs::path(argv[2]) / L"pinned-readback.gz";
        fs::copy_file(fs::path(argv[2]) / L"valid.gz", primary, fs::copy_options::overwrite_existing);
        check(pinned_snapshot(primary, gzip, expected));
        {
            Handle writer(CreateFileW(primary.c_str(), GENERIC_WRITE, FILE_SHARE_READ, nullptr, OPEN_EXISTING, 0, nullptr));
            check(writer.value != INVALID_HANDLE_VALUE);
            rejected = false;
            try { pinned_snapshot(primary, gzip, expected); } catch (const std::runtime_error&) { rejected = true; }
            check(rejected);
        }
        {
            auto ancestors = pin_directories(primary.parent_path());
            auto file = open_file(primary, GENERIC_READ, OPEN_EXISTING, FILE_SHARE_READ);
            Handle writer(CreateFileW(primary.c_str(), GENERIC_WRITE, FILE_SHARE_READ, nullptr, OPEN_EXISTING, 0, nullptr));
            check(writer.value == INVALID_HANDLE_VALUE && GetLastError() == ERROR_SHARING_VIOLATION);
            check(!DeleteFileW(primary.c_str()) && GetLastError() == ERROR_SHARING_VIOLATION);
        }
        const fs::path alias = fs::path(argv[2]) / L"linked-readback.gz";
        check(CreateHardLinkW(alias.c_str(), primary.c_str(), nullptr) != FALSE);
        rejected = false;
        try { pinned_snapshot(primary, gzip, expected); } catch (const std::runtime_error&) { rejected = true; }
        check(rejected);
        check(DeleteFileW(alias.c_str()) != FALSE);
        check(DeleteFileW(primary.c_str()) != FALSE);
        FreeLibrary(library);
        std::cout << "Nightingale owned snapshot: " << checks << " checks passed\n";
        return 0;
    } catch (const std::exception& error) { std::cerr << error.what() << '\n'; return 1; }
}
