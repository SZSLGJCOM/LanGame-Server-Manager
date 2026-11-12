#include <API/ARK/Ark.h>
#include <json.hpp>

#include <cmath>
#include <cstdint>
#include <limits>
#include <mutex>
#include <sstream>
#include <stdexcept>
#include <string>
#include <unordered_map>
#include <vector>

#ifdef LGSM_ASA
namespace ServerApi = AsaApi;
constexpr const char* Edition = "asa";
#else
namespace ServerApi = ArkApi;
constexpr const char* Edition = "ase";
#endif

namespace {
using Json = nlohmann::json;
struct Receipt { std::string command; Json result; };
// RCON callbacks execute in the server's world tick. Keep receipts until unload;
// rejecting a full table is safer than silently reusing an evicted mutation ID.
std::mutex receipt_mutex;
std::unordered_map<std::string, Receipt> receipts;

Json Error(const std::string& id, const char* code) {
    return {{"version", 1}, {"edition", Edition}, {"ok", false},
            {"requestId", id}, {"error", code}};
}

bool Token(const std::string& value, std::size_t limit) {
    if (value.empty() || value.size() > limit) return false;
    for (unsigned char c : value) {
        if (!((c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') ||
              (c >= '0' && c <= '9') || c == '-' || c == '_')) return false;
    }
    return true;
}

std::uint32_t UInt(const std::string& value) {
    if (value.empty() || value.size() > 10) throw std::invalid_argument("integer");
    std::uint64_t result = 0;
    for (unsigned char c : value) {
        if (c < '0' || c > '9') throw std::invalid_argument("integer");
        result = result * 10 + c - '0';
    }
    if (result > std::numeric_limits<std::uint32_t>::max())
        throw std::invalid_argument("integer");
    return static_cast<std::uint32_t>(result);
}

double Coordinate(const std::string& value) {
    std::size_t consumed = 0;
    const double result = std::stod(value, &consumed);
    if (consumed != value.size() || !std::isfinite(result) || std::abs(result) > 10000000)
        throw std::invalid_argument("coordinate");
    return result;
}

bool Blueprint(const std::string& value) {
    const bool path = value.rfind("/Game/", 0) == 0;
    if (value.size() > 300 ||
        (!path && !Token(value, 300)) ||
        !value.ends_with("_C") || value.find("..") != std::string::npos ||
        value.find("//") != std::string::npos) return false;
    for (unsigned char c : value) {
        if (!((c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') ||
              (c >= '0' && c <= '9') || c == '/' || c == '.' || c == '_' ||
              (path && c == '-'))) return false;
    }
    return true;
}

UClass* ResolveClass(const std::string& value) {
    if (value.rfind("/Game/", 0) == 0) return UVictoryCore::BPLoadClass(FString(value.c_str()));
#ifdef LGSM_ASA
    const std::wstring name(value.begin(), value.end());
    return static_cast<UClass*>(Globals::StaticFindObject(UClass::GetPrivateStaticClass(),
        reinterpret_cast<UObject*>(-1), name.c_str(), false));
#else
    auto& objects = Globals::GUObjectArray()().ObjObjects;
    UClass* match = nullptr;
    for (int i = 0; i < objects.NumElements; ++i) {
        auto* item = objects.GetObjectPtr(i);
        auto* object = item ? item->Object : nullptr;
        if (!object || !object->IsA(UClass::GetPrivateStaticClass()) ||
            object->NameField().ToString().ToString() != value) continue;
        if (match) return nullptr; // An ambiguous mod class needs its full path.
        match = static_cast<UClass*>(object);
    }
    return match;
#endif
}

bool IsTamed(APrimalDinoCharacter* dino) {
#ifdef LGSM_ASA
    auto* function = dino->ClassField()->FindFunctionByName("BPIsTamed", EIncludeSuperFlag::IncludeSuper);
    if (!function) throw std::runtime_error("tame_state_unavailable");
    bool result = false;
    dino->ProcessEvent(function, &result);
    return result;
#else
    return dino->BPIsTamed();
#endif
}

AShooterPlayerController* Owner(UWorld* world, std::uint32_t player_id, Json& diagnostics) {
    diagnostics["ownerFailure"] = "not_online";
    for (auto& weak : world->PlayerControllerListField()) {
        auto* controller = weak.Get();
        if (!controller || !controller->IsA(AShooterPlayerController::GetPrivateStaticClass()))
            continue;
        auto* player = static_cast<AShooterPlayerController*>(controller);
        if (player->LinkedPlayerIDField() != player_id) continue;
        if (!player->GetPlayerCharacter()) diagnostics["ownerFailure"] = "character_unavailable";
        else if (!player->PlayerStateField()) diagnostics["ownerFailure"] = "player_state_unavailable";
        else if (player->TargetingTeamField() <= 0) diagnostics["ownerFailure"] = "team_unavailable";
        else {
            diagnostics.erase("ownerFailure");
            diagnostics["ownerTeam"] = player->TargetingTeamField();
            return player;
        }
        return nullptr;
    }
    return nullptr;
}

#include "PlayerMap.h"

Json InspectDino(const std::string& id, APrimalDinoCharacter* dino) {
    if (!dino || dino->IsDead()) return Error(id, "entity_not_found");
#ifdef LGSM_ASA
    if (!dino->RootComponentField().Get()) return Error(id, "entity_not_initialized");
    const auto position = dino->GetLocation();
#else
    FVector position;
    if (!dino->RootComponentField()) return Error(id, "entity_not_initialized");
    dino->RootComponentField()->GetWorldLocation(&position);
#endif
    auto* status = dino->MyCharacterStatusComponentField();
    if (!status) return Error(id, "entity_not_initialized");
    return {{"version", 1}, {"edition", Edition}, {"ok", true}, {"requestId", id},
            {"id1", dino->DinoID1Field()}, {"id2", dino->DinoID2Field()},
            {"className", dino->ClassField()->NameField().ToString().ToString()},
            {"level", status->BaseCharacterLevelField()}, {"tamed", IsTamed(dino)},
            {"team", dino->TargetingTeamField()}, {"x", position.X},
            {"y", position.Y}, {"z", position.Z}};
}

Json Spawn(UWorld* world, const std::vector<std::string>& args) {
    const std::string& id = args[1];
    Json diagnostics = {{"stage", "validate"}, {"cleanup", "not_needed"}};
    APrimalDinoCharacter* dino = nullptr;
    const auto failed = [&](Json result) {
        if (dino) {
            // Destroy's true result means destruction was requested, not that
            // the world has already removed the actor or saved the change.
            try { diagnostics["cleanup"] = dino->Destroy(true, false) ? "destroy_requested" : "destroy_failed"; }
            catch (const std::exception&) { diagnostics["cleanup"] = "destroy_failed"; }
        }
        result["diagnostics"] = diagnostics;
        return result;
    };
    try {
        if (args.size() != 9 || !Blueprint(args[2])) return failed(Error(id, "invalid_arguments"));
        const auto level = UInt(args[3]);
        const double x = Coordinate(args[4]), y = Coordinate(args[5]), z = Coordinate(args[6]);
        const bool tame = args[7] == "tamed";
        if ((!tame && args[7] != "wild") || level < 1 || level > 5000)
            return failed(Error(id, "invalid_arguments"));
        const auto player_id = UInt(args[8]);
        if (!tame && player_id != 0) return failed(Error(id, "invalid_arguments"));
        diagnostics["stage"] = "owner";
        auto* owner = tame ? Owner(world, player_id, diagnostics) : nullptr;
        if (tame && !owner) return failed(Error(id, "owner_not_online"));
        diagnostics["stage"] = "class";
        auto* cls = ResolveClass(args[2]);
        if (!cls || !cls->IsChildOf(APrimalDinoCharacter::GetPrivateStaticClass()))
            return failed(Error(id, "invalid_dino_class"));
        TSubclassOf<APrimalDinoCharacter> dino_class;
        dino_class.uClass = cls;
        const FVector position{static_cast<decltype(FVector::X)>(x),
                               static_cast<decltype(FVector::Y)>(y),
                               static_cast<decltype(FVector::Z)>(z)};
        const FRotator rotation{0, 0, 0};
        diagnostics["stage"] = "spawn";
        dino = APrimalDinoCharacter::SpawnDino(world, dino_class, position, rotation,
            1.0f, 0, false, true, static_cast<int>(level), false, 1.0f,
            static_cast<int>(level), true
#ifdef LGSM_ASA
            , false, false
#endif
        );
        if (!dino) return failed(Error(id, "spawn_failed"));
        diagnostics["id1"] = dino->DinoID1Field();
        diagnostics["id2"] = dino->DinoID2Field();
        if (tame) {
            diagnostics["stage"] = "taming_setup";
            // TameDino consumes the taming team and tribe settings; the
            // controller argument alone does not establish ownership.
#ifdef LGSM_ASA
            auto* state = static_cast<AShooterPlayerState*>(owner->PlayerStateField().Get());
#else
            auto* state = static_cast<AShooterPlayerState*>(owner->PlayerStateField());
#endif
            dino->TamingTeamIDField() = owner->TargetingTeamField();
            dino->TamerStringField() = ServerApi::IApiUtils::GetCharacterName(owner);
            state->SetTribeTamingDinoSettings(dino);
            diagnostics["stage"] = "tame";
            dino->TameDino(owner, true, 0, true, true, true);
            diagnostics["stage"] = "tame_verify";
            const bool tamed = IsTamed(dino);
            diagnostics["actualTamed"] = tamed;
            diagnostics["actualTeam"] = dino->TargetingTeamField();
            if (!tamed || dino->TargetingTeamField() != owner->TargetingTeamField()) {
                return failed(Error(id, "tame_failed"));
            }
        }
        diagnostics["stage"] = "identity";
        const auto id1 = dino->DinoID1Field(), id2 = dino->DinoID2Field();
        diagnostics["id1"] = id1;
        diagnostics["id2"] = id2;
        if ((!id1 && !id2) || APrimalDinoCharacter::FindDinoWithID(world, id1, id2) != dino) {
            return failed(Error(id, "entity_readback_failed"));
        }
        diagnostics["stage"] = "inspect";
        auto result = InspectDino(id, dino);
        if (!result.at("ok").get<bool>()) return failed(std::move(result));
        return result;
    } catch (const std::invalid_argument&) {
        return failed(Error(id, dino ? "entity_readback_failed" : "invalid_arguments"));
    } catch (const std::exception&) {
        return failed(Error(id, dino ? "entity_readback_failed" : "outcome_unknown"));
    }
}

void Execute(RCONClientConnection* connection, RCONPacket* packet, UWorld* world) {
    if (!connection || !packet || !connection->IsAuthenticatedField()) return;
    std::string id;
    std::string action;
    Json result;
    try {
        const std::string body = packet->Body.ToString();
        if (body.size() > 768) throw std::invalid_argument("length");
        std::istringstream stream(body);
        std::vector<std::string> args;
        for (std::string arg; stream >> arg;) args.push_back(std::move(arg));
        if (args.empty()) throw std::invalid_argument("command");
        action = args[0] == "LgsmArkTools.Status" ? "status" :
                 args[0] == "LgsmArkTools.Players" ? "players" :
                 args[0] == "LgsmArkTools.Spawn" ? "spawn" : "inspect";
        if (args.size() < 2 || args[1].size() != 32 || args[1].find_first_not_of("0123456789abcdefABCDEF") != std::string::npos)
            throw std::invalid_argument("requestId");
        id = args[1];
        if (args[0] == "LgsmArkTools.Status" && args.size() == 2) {
            result = {{"version", 1}, {"edition", Edition}, {"ok", world != nullptr}, {"requestId", id}};
        } else {
            if (!world) result = Error(id, "world_not_ready");
            else if (args[0] == "LgsmArkTools.Players" && args.size() == 2)
                result = PlayerMap(world, id);
            else if (args[0] == "LgsmArkTools.Inspect" && args.size() == 4)
                result = InspectDino(id, APrimalDinoCharacter::FindDinoWithID(world, UInt(args[2]), UInt(args[3])));
            else if (args[0] == "LgsmArkTools.Spawn") {
                std::lock_guard lock(receipt_mutex);
                const auto found = receipts.find(id);
                if (found != receipts.end())
                    result = found->second.command == body ? found->second.result : Error(id, "request_id_conflict");
                else if (receipts.size() >= 512) result = Error(id, "request_capacity_reached");
                else {
                    auto& receipt = receipts.emplace(id, Receipt{body, Error(id, "outcome_unknown")}).first->second;
                    try { result = Spawn(world, args); }
                    catch (const std::invalid_argument&) { result = Error(id, "invalid_arguments"); }
                    catch (const std::exception&) { result = Error(id, "outcome_unknown"); }
                    receipt.result = result;
                }
            } else result = Error(id, "invalid_arguments");
        }
    } catch (const std::invalid_argument&) {
        result = Error(id, "invalid_arguments");
    } catch (const std::exception&) {
        result = Error(id, "native_operation_failed");
    }
    result["action"] = action;
    if (action == "spawn") {
        // Persist the bounded receipt even if the RCON peer disconnects. It
        // contains generated-entity identity and fixed diagnostics, never the
        // raw command, account names, credentials or arbitrary exception text.
        const auto receipt = "LGSM_ARK_TOOLS_RECEIPT " + result.dump(-1, ' ', true, Json::error_handler_t::replace) + "\r\n";
        DWORD written{};
        WriteFile(GetStdHandle(STD_ERROR_HANDLE), receipt.data(), static_cast<DWORD>(receipt.size()), &written, nullptr);
    }
    // The two SDKs use different narrow-string -> FString conversions. ASCII
    // JSON with Unicode escapes preserves player names through both engines.
    auto response = "LGSM_ARK_TOOLS " + result.dump(-1, ' ', true, Json::error_handler_t::replace) + "\n";
    if (response.size() > 400 * 1024) {
        result = Error(id, "response_limit_exceeded");
        result["action"] = action;
        response = "LGSM_ARK_TOOLS " + result.dump() + "\n";
    }
    // Source RCON has bounded frames. Keep one JSON envelope, split only at
    // UTF-8 boundaries, and fit the host's 64-frame completion budget.
    for (std::size_t offset = 0; offset < response.size();) {
        auto end = (std::min)(offset + 8192, response.size());
        while (end < response.size() && (static_cast<unsigned char>(response[end]) & 0xc0) == 0x80) --end;
        FString text(response.substr(offset, end - offset).c_str());
        connection->SendMessageW(packet->Id, 0, &text);
        offset = end;
    }
}

void Load() {
    for (const auto* command : {"LgsmArkTools.Status", "LgsmArkTools.Spawn", "LgsmArkTools.Inspect", "LgsmArkTools.Players"})
        ServerApi::GetCommands().AddRconCommand(command, Execute);
}
void Unload() {
    for (const auto* command : {"LgsmArkTools.Status", "LgsmArkTools.Spawn", "LgsmArkTools.Inspect", "LgsmArkTools.Players"})
        ServerApi::GetCommands().RemoveRconCommand(command);
}
}

#ifdef LGSM_ASA
extern "C" __declspec(dllexport) void Plugin_Init() { Load(); }
extern "C" __declspec(dllexport) void Plugin_Unload() { Unload(); }
#else
BOOL APIENTRY DllMain(HMODULE, DWORD reason, LPVOID) {
    if (reason == DLL_PROCESS_ATTACH) Load();
    else if (reason == DLL_PROCESS_DETACH) Unload();
    return TRUE;
}
#endif
