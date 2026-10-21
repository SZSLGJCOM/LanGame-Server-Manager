#pragma once
#include <windows.h>
#include <bcrypt.h>
#include <array>
#include <cstddef>
#include <cstdint>
#include <span>
#include <stdexcept>

namespace nightingale_save {
inline constexpr uint64_t MAX_BYTES = 128ULL * 1024 * 1024;
inline void ensure(bool value) { if (!value) throw std::runtime_error("snapshot_unavailable"); }

class Sha256 {
    BCRYPT_ALG_HANDLE algorithm{};
    BCRYPT_HASH_HANDLE hash{};
public:
    Sha256() {
        ensure(BCryptOpenAlgorithmProvider(&algorithm, BCRYPT_SHA256_ALGORITHM, nullptr, 0) >= 0);
        if (BCryptCreateHash(algorithm, &hash, nullptr, 0, nullptr, 0, 0) < 0) {
            BCryptCloseAlgorithmProvider(algorithm, 0); algorithm = nullptr;
            throw std::runtime_error("snapshot_hash");
        }
    }
    Sha256(const Sha256&) = delete;
    ~Sha256() { if (hash) BCryptDestroyHash(hash); if (algorithm) BCryptCloseAlgorithmProvider(algorithm, 0); }
    void update(std::span<const uint8_t> data) {
        ensure(data.size() <= MAX_BYTES);
        ensure(BCryptHashData(hash, const_cast<PUCHAR>(data.data()), static_cast<ULONG>(data.size()), 0) >= 0);
    }
    std::array<uint8_t, 32> finish() {
        std::array<uint8_t, 32> result{};
        ensure(BCryptFinishHash(hash, result.data(), static_cast<ULONG>(result.size()), 0) >= 0);
        return result;
    }
};
struct Content {
    uint64_t length{};
    std::array<uint8_t, 32> hash{};
    static Content capture(std::span<const uint8_t> bytes) {
        ensure(!bytes.empty() && bytes.size() <= MAX_BYTES);
        Sha256 digest; digest.update(bytes); return {bytes.size(), digest.finish()};
    }
};
enum class SnapshotIssue { none, unavailable, gzip_shape, initialize, inflate, no_progress, trailing_bytes, length, hash };
inline const char* issue_code(SnapshotIssue issue) {
    switch (issue) {
        case SnapshotIssue::none: return "snapshot_matches";
        case SnapshotIssue::unavailable: return "snapshot_unavailable";
        case SnapshotIssue::gzip_shape: return "snapshot_gzip_shape";
        case SnapshotIssue::initialize: return "snapshot_gzip_initialize";
        case SnapshotIssue::inflate: return "snapshot_gzip_decode";
        case SnapshotIssue::no_progress: return "snapshot_gzip_no_progress";
        case SnapshotIssue::trailing_bytes: return "snapshot_gzip_trailing_bytes";
        case SnapshotIssue::length: return "snapshot_length_mismatch";
        case SnapshotIssue::hash: return "snapshot_hash_mismatch";
    }
    return "snapshot_unavailable";
}

// Public zlib 1.3 C ABI, Windows x64 (uLong is 32 bits). The game's own
// appUncompressMemoryGZIP passes this same 0x58 layout to inflateInit2_.
// API: https://github.com/madler/zlib/blob/v1.3/zlib.h (Gailly / Adler).
struct InflateStream {
    const uint8_t* input{}; uint32_t available_input{}, total_input{};
    uint8_t* output{}; uint32_t available_output{}, total_output{};
    const char* message{}; void* state{};
    void* (*allocate)(void*, uint32_t, uint32_t){};
    void (*release)(void*, void*){};
    void* opaque{}; int data_type{}; uint32_t checksum{}, reserved{};
};
static_assert(sizeof(InflateStream) == 0x58 && offsetof(InflateStream, state) == 0x28);
struct Gzip {
    int (*initialize)(InflateStream*, int, const char*, int){};
    int (*inflate)(InflateStream*, int){};
    int (*finish)(InflateStream*){};
    const char* version{};

    SnapshotIssue verify(std::span<const uint8_t> compressed, const Content& expected) const {
        if (!initialize || !inflate || !finish || !version || compressed.size() < 18
            || compressed.size() > MAX_BYTES || !expected.length || expected.length > MAX_BYTES) return SnapshotIssue::unavailable;
        if (compressed[0] != 0x1f || compressed[1] != 0x8b || compressed[2] != 8) return SnapshotIssue::gzip_shape;
        InflateStream stream{};
        if (initialize(&stream, 31, version, sizeof(stream)) != 0) return SnapshotIssue::initialize;
        struct End { const Gzip& codec; InflateStream& stream; ~End() { codec.finish(&stream); } } end{*this, stream};
        stream.input = compressed.data(); stream.available_input = static_cast<uint32_t>(compressed.size());
        Sha256 digest;
        std::array<uint8_t, 65536> output{};
        uint64_t length{};
        for (;;) {
            stream.output = output.data(); stream.available_output = static_cast<uint32_t>(output.size());
            const auto previous_input = stream.total_input;
            const int result = inflate(&stream, 0);
            const auto count = output.size() - stream.available_output;
            length += count;
            if (length > expected.length || length > MAX_BYTES) return SnapshotIssue::length;
            digest.update(std::span(output.data(), count));
            if (result == 1) {
                // CRC/ISIZE must pass, exactly one complete gzip member must
                // consume the entire pinned file, and all owned bytes match.
                if (stream.available_input || stream.total_input != compressed.size()) return SnapshotIssue::trailing_bytes;
                if (stream.total_output != length || length != expected.length) return SnapshotIssue::length;
                return digest.finish() == expected.hash ? SnapshotIssue::none : SnapshotIssue::hash;
            }
            if (result != 0) return SnapshotIssue::inflate;
            if (!count && stream.total_input == previous_input) return SnapshotIssue::no_progress;
        }
    }
    bool matches(std::span<const uint8_t> compressed, const Content& expected) const {
        return verify(compressed, expected) == SnapshotIssue::none;
    }
};
}
