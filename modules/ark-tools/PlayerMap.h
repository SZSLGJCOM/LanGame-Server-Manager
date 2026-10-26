#pragma once

// Included after the pinned edition SDK and the plugin's protocol helpers.
// Native fields follow Ark/ArkApiUtils.h::FVectorToCoords. Multi-layer ASA maps
// deliberately use world coordinates: different layers cannot share one chart.
Json MapProjection(UWorld* world, const std::string& map_name) {
    auto* native_settings = world->GetWorldSettings(false, false);
    if (!native_settings) return nullptr;
#ifdef LGSM_ASA
    auto* settings_class = APrimalWorldSettings::GetPrivateStaticClass();
#else
    static UClass* settings_class = nullptr;
    if (!settings_class) settings_class = ResolveClass("PrimalWorldSettings");
#endif
    if (!settings_class || !native_settings->IsA(settings_class)) return nullptr;
    auto* settings = static_cast<APrimalWorldSettings*>(native_settings);
    double min_x, max_x, min_y, max_y;
#ifdef LGSM_ASA
    auto* cls = settings->CurrentMinimapDataField().uClass;
    if (!cls) cls = UVictoryCore::BPLoadClass("Blueprint'/Game/ASA/Minimap/Core/MinimapData_Base.MinimapData_Base'");
    if (!cls) return nullptr;
    auto* data = static_cast<UMinimapData*>(cls->GetDefaultObject(true));
    if (!data || data->MinimapDataField().Num() != 1) return nullptr;
    auto* map = data->MinimapDataField().GetData();
    min_x = map->OriginMinField().X; max_x = map->OriginMaxField().X;
    min_y = map->OriginMinField().Y; max_y = map->OriginMaxField().Y;
#else
    double lat_scale = settings->LatitudeScaleField();
    double lon_scale = settings->LongitudeScaleField();
    double lat_origin = settings->LatitudeOriginField();
    double lon_origin = settings->LongitudeOriginField();
    // The stock Island predates explicit projection fields. Do not apply its
    // legacy dimensions to an unknown mod map with missing metadata.
    if (lat_scale == 0 || lon_scale == 0 || lat_origin == 0 || lon_origin == 0) {
        if (map_name != "TheIsland") return nullptr;
        if (lat_scale == 0) lat_scale = 800.0;
        if (lon_scale == 0) lon_scale = 800.0;
        if (lat_origin == 0) lat_origin = -400000.0;
        if (lon_origin == 0) lon_origin = -400000.0;
    }
    min_x = -std::abs(lon_origin); max_x = min_x + lon_scale * 1000.0;
    min_y = -std::abs(lat_origin); max_y = min_y + lat_scale * 1000.0;
#endif
    for (double value : {min_x, max_x, min_y, max_y})
        if (!std::isfinite(value) || std::abs(value) > 10000000) return nullptr;
    if (max_x <= min_x || max_y <= min_y) return nullptr;
    return {{"minX", min_x}, {"maxX", max_x}, {"minY", min_y}, {"maxY", max_y}};
}

std::string MapPlayerText(std::string value, std::size_t maximum) {
    if (value.size() > maximum) throw std::runtime_error("player_text_limit");
    for (char& c : value) if (static_cast<unsigned char>(c) < 32 || c == 127) c = ' ';
    return value;
}

Json PlayerMap(UWorld* world, const std::string& id) {
    if (ServerApi::GetApiUtils().GetStatus() != ServerApi::ServerStatus::Ready)
        return Error(id, "world_not_ready");
    FString map;
    world->GetMapName(&map);
    const auto map_name = map.ToString();
    if (map_name.empty() || map_name.size() > 128) return Error(id, "map_not_ready");
    Json players = Json::array();
    for (auto& weak : world->PlayerControllerListField()) {
        auto* controller = weak.Get();
        if (!controller || !controller->IsA(AShooterPlayerController::GetPrivateStaticClass())) continue;
        auto* player = static_cast<AShooterPlayerController*>(controller);
        const auto player_id = player->LinkedPlayerIDField();
        if (!player_id || !player->PlayerStateField()) continue; // Not yet authenticated/spawned.
        if (players.size() >= 512) return Error(id, "player_limit_exceeded");
        auto native_name = ServerApi::IApiUtils::GetCharacterName(player);
#ifdef LGSM_ASA
        auto name = native_name.ToStringUTF8();
        auto account = ServerApi::IApiUtils::GetEOSIDFromController(player).ToString();
#else
        auto name = native_name.ToString(); // The ASE SDK uses CP_UTF8 here.
        const auto steam_id = ServerApi::IApiUtils::GetSteamIdFromController(player);
        auto account = steam_id ? std::to_string(steam_id) : std::string();
#endif
        Json record = {{"playerId", player_id}, {"name", MapPlayerText(name, 1024)},
            {"accountId", MapPlayerText(account, 256)}, {"team", player->TargetingTeamField()},
            {"x", nullptr}, {"y", nullptr}, {"z", nullptr}};
        auto* character = player->GetPlayerCharacter();
        if (character && character->RootComponentField()) {
#ifdef LGSM_ASA
            const auto position = character->GetLocation();
#else
            FVector position;
            character->RootComponentField()->GetWorldLocation(&position);
#endif
            if (std::isfinite(position.X) && std::isfinite(position.Y) && std::isfinite(position.Z)
                && std::abs(position.X) <= 10000000 && std::abs(position.Y) <= 10000000 && std::abs(position.Z) <= 10000000) {
                record["x"] = position.X; record["y"] = position.Y; record["z"] = position.Z;
            }
        }
        players.push_back(std::move(record));
    }
    return {{"version", 1}, {"edition", Edition}, {"ok", true}, {"requestId", id},
        {"mapName", map_name}, {"projection", MapProjection(world, map_name)}, {"players", std::move(players)}};
}
