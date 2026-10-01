use super::super::*;
use super::ark_ascended_support::{ark_ascended_test_descriptor, prepare_ark_ascended_environment};

#[tokio::test]
async fn ark_ascended_instance_renders_windows_first_server_files() {
    let root = unique_test_root();
    let paths = test_paths(&root);
    let descriptor = ark_ascended_test_descriptor(&root);
    prepare_ark_ascended_environment(&root, &descriptor);

    initialize_database(&paths).await.unwrap();
    sync_modules(&paths, std::slice::from_ref(&descriptor))
        .await
        .unwrap();

    let install_root = root.join("verified-installs").join("arksa");
    fs::create_dir_all(&install_root).unwrap();
    sync_game_installs(
        &paths,
        &[GameInstallSyncRecord {
            module_id: String::from("arksurvivalascended"),
            install_root: install_root.to_string_lossy().into_owned(),
            install_state: InstallState::Installed,
            current_version: Some(String::from("arksa-test-build")),
            mark_verified: true,
        }],
    )
    .await
    .unwrap();

    let created = create_instance(
        &paths,
        &descriptor,
        CreateInstanceInput {
            name: String::from("ASA Alpha"),
            module_id: String::from("arksurvivalascended"),
        },
    )
    .await
    .unwrap();

    assert_eq!(created.summary.port_count, 4);

    let details =
        configure_test_instance_runtime(&paths, &created.summary.id, "192.168.50.20", false).await;
    let expected_saves_root = instance_private_runtime_root(&created)
        .join("ShooterGame")
        .join("Saved");
    assert_eq!(PathBuf::from(&details.saves_path), expected_saves_root);
    assert!(details.backup_uses_declared_saves_path);

    let config_root = root
        .join("instances")
        .join(&created.summary.id)
        .join("config");
    let game_user_settings_text =
        fs::read_to_string(config_root.join("GameUserSettings.ini")).unwrap();
    let game_ini_text = fs::read_to_string(config_root.join("Game.ini")).unwrap();
    let live_saved_root = instance_private_runtime_root(&created)
        .join("ShooterGame")
        .join("Saved");
    let live_config_root = live_saved_root.join("Config").join("WindowsServer");
    let live_binary_root = instance_private_runtime_root(&created)
        .join("ShooterGame")
        .join("Binaries")
        .join("Win64");
    let admin_ids_text =
        fs::read_to_string(config_root.join("AllowedCheaterAccountIDs.txt")).unwrap();
    let exclusive_list_text =
        fs::read_to_string(config_root.join("PlayersExclusiveJoinList.txt")).unwrap();

    assert!(game_user_settings_text.contains("ServerAdminPassword=asa-admin"));
    assert!(game_user_settings_text.contains("RCONEnabled=true"));
    assert!(game_user_settings_text.contains("TribeLogDestroyedEnemyStructures=true"));
    assert!(game_user_settings_text.contains("AllowHideDamageSourceFromLogs=false"));
    assert!(game_user_settings_text.contains("AutoSavePeriodMinutes=20.0"));
    assert!(game_user_settings_text.contains("BanListURL=\"https://example.com/asa-banlist.txt\""));
    assert!(
        game_user_settings_text
            .contains("CustomDynamicConfigUrl=\"http://cdn.example.com/asa-dynamic.ini\"")
    );
    assert!(game_user_settings_text.contains(
        "CustomLiveTuningUrl=\"https://cdn2.arkdedicated.com/asa/livetuningoverloads.json\""
    ));
    assert!(game_user_settings_text.contains("ShowFloatingDamageText=true"));
    assert!(game_user_settings_text.contains("ServerHardcore=true"));
    assert!(game_user_settings_text.contains("ServerForceNoHUD=false"));
    assert!(game_user_settings_text.contains("SpectatorPassword=fixture-observer-mode"));
    assert!(game_user_settings_text.contains("KickIdlePlayersPeriod=2700.0"));
    assert!(game_user_settings_text.contains("StructurePickupTimeAfterPlacement=45.0"));
    assert!(game_user_settings_text.contains("StructurePickupHoldDuration=0.75"));
    assert!(game_user_settings_text.contains("TheMaxStructuresInRange=9500"));
    assert!(game_user_settings_text.contains("MaxPlatformSaddleStructureLimit=100"));
    assert!(game_user_settings_text.contains("AutoDestroyOldStructuresMultiplier=0.5"));
    assert!(game_user_settings_text.contains("OnlyAutoDestroyCoreStructures=true"));
    assert!(game_user_settings_text.contains("OnlyDecayUnsnappedCoreStructures=true"));
    assert!(game_user_settings_text.contains("FastDecayUnsnappedCoreStructures=true"));
    assert!(game_user_settings_text.contains("DestroyTamesOverTheSoftTameLimit=true"));
    assert!(game_user_settings_text.contains("MaxTamedDinos_SoftTameLimit=4500"));
    assert!(
        game_user_settings_text
            .contains("MaxTamedDinos_SoftTameLimit_CountdownForDeletionDuration=259200")
    );
    assert!(game_user_settings_text.contains("NoTributeDownloads=true"));
    assert!(game_user_settings_text.contains("PreventDownloadSurvivors=true"));
    assert!(game_user_settings_text.contains("PreventDownloadItems=false"));
    assert!(game_user_settings_text.contains("PreventDownloadDinos=true"));
    assert!(game_user_settings_text.contains("PreventUploadSurvivors=false"));
    assert!(game_user_settings_text.contains("PreventUploadItems=true"));
    assert!(game_user_settings_text.contains("PreventUploadDinos=true"));
    assert!(game_user_settings_text.contains("MinimumDinoReuploadInterval=900.0"));
    assert!(game_user_settings_text.contains("MaxTributeCharacters=15"));
    assert!(game_user_settings_text.contains("MaxTributeDinos=40"));
    assert!(game_user_settings_text.contains("MaxTributeItems=100"));
    assert!(game_user_settings_text.contains("TributeCharacterExpirationSeconds=172800"));
    assert!(game_user_settings_text.contains("TributeDinoExpirationSeconds=259200"));
    assert!(game_user_settings_text.contains("TributeItemExpirationSeconds=432000"));
    assert!(game_user_settings_text.contains("bFilterChat=true"));
    assert!(game_user_settings_text.contains("bFilterCharacterNames=true"));
    assert!(game_user_settings_text.contains("bFilterTribeNames=true"));
    assert!(
        game_user_settings_text.contains("BadWordListURL=http://arkdedicated.com/badwords.txt")
    );
    assert!(game_user_settings_text.contains("BadWordWhiteListURL="));
    assert!(game_user_settings_text.contains("AllowFlyingStaminaRecovery=true"));
    assert!(game_user_settings_text.contains("AllowMultipleAttachedC4=true"));
    assert!(game_user_settings_text.contains("AllowRaidDinoFeeding=true"));
    assert!(game_user_settings_text.contains("AllowHitMarkers=true"));
    assert!(game_user_settings_text.contains("DisableWeatherFog=true"));
    assert!(game_user_settings_text.contains("EnablePvPGamma=true"));
    assert!(game_user_settings_text.contains("DisablePvEGamma=false"));
    assert!(game_user_settings_text.contains("UseExclusiveList=true"));
    assert!(game_user_settings_text.contains("AllowCaveBuildingPvE=true"));
    assert!(game_user_settings_text.contains("DisableStructureDecayPvE=true"));
    assert!(game_user_settings_text.contains("PreventOfflinePvP=true"));
    assert!(game_user_settings_text.contains("PreventOfflinePvPInterval=900.0"));
    assert!(game_user_settings_text.contains("OverrideStructurePlatformPrevention=true"));
    assert!(game_user_settings_text.contains("EnableExtraStructurePreventionVolumes=true"));
    assert!(game_user_settings_text.contains("PvEAllowStructuresAtSupplyDrops=false"));
    assert!(game_user_settings_text.contains("AllowCrateSpawnsOnTopOfStructures=true"));
    assert!(game_user_settings_text.contains("ForceAllStructureLocking=true"));
    assert!(game_user_settings_text.contains("DisableDinoDecayPvE=true"));
    assert!(game_user_settings_text.contains("AutoDestroyDecayedDinos=true"));
    assert!(game_user_settings_text.contains("PvEStructureDecayPeriodMultiplier=1.5"));
    assert!(game_user_settings_text.contains("PvEDinoDecayPeriodMultiplier=2.0"));
    assert!(game_user_settings_text.contains("AllowIntegratedSPlusStructures=true"));
    assert!(game_user_settings_text.contains("ResourcesRespawnPeriodMultiplier=1.25"));
    assert!(game_user_settings_text.contains("ItemStackSizeMultiplier=3.0"));
    assert!(game_user_settings_text.contains("DinoCountMultiplier=1.35"));
    assert!(game_user_settings_text.contains("ServerAutoForceRespawnWildDinosInterval=604800.0"));
    assert!(game_user_settings_text.contains("PreventDiseases=true"));
    assert!(game_user_settings_text.contains("bForceCanRideFliers=true"));
    assert!(game_user_settings_text.contains("PlayerDamageMultiplier=1.2"));
    assert!(game_user_settings_text.contains("PlayerResistanceMultiplier=0.85"));
    assert!(game_user_settings_text.contains("DinoDamageMultiplier=1.15"));
    assert!(game_user_settings_text.contains("DinoResistanceMultiplier=0.9"));
    assert!(game_user_settings_text.contains("TamedDinoDamageMultiplier=1.3"));
    assert!(game_user_settings_text.contains("TamedDinoResistanceMultiplier=0.8"));
    assert!(game_user_settings_text.contains("StructureDamageMultiplier=1.1"));
    assert!(game_user_settings_text.contains("StructureResistanceMultiplier=0.75"));
    assert!(game_user_settings_text.contains("EnableCryopodNerf=true"));
    assert!(game_user_settings_text.contains("CryopodNerfDuration=600.0"));
    assert!(game_user_settings_text.contains("CryopodNerfDamageMult=0.01"));
    assert!(game_user_settings_text.contains("CryopodNerfIncomingDamageMultPercent=25.0"));
    assert!(game_user_settings_text.contains("EnableCryoSicknessPVE=true"));
    assert!(game_user_settings_text.contains("DisableCryopodEnemyCheck=true"));
    assert!(game_user_settings_text.contains("DisableCryopodFridgeRequirement=false"));
    assert!(game_user_settings_text.contains("AllowCryoFridgeOnSaddle=true"));
    assert!(game_user_settings_text.contains("SessionName=ASA Alpha"));
    assert!(game_user_settings_text.contains("Port=7777"));
    assert!(game_user_settings_text.contains("QueryPort=27015"));
    assert!(game_user_settings_text.contains("MaxPlayers=30"));

    assert!(game_ini_text.contains("ResourceNoReplenishRadiusPlayers=0.5"));
    assert!(game_ini_text.contains("ResourceNoReplenishRadiusStructures=0.5"));
    assert!(game_ini_text.contains("CropGrowthSpeedMultiplier=2.0"));
    assert!(game_ini_text.contains("CropDecaySpeedMultiplier=0.5"));
    assert!(game_ini_text.contains("PoopIntervalMultiplier=0.75"));
    assert!(game_ini_text.contains("PerLevelStatsMultiplier_Player[0]=1.5"));
    assert!(game_ini_text.contains("PerLevelStatsMultiplier_Player[7]=2.0"));
    assert!(game_ini_text.contains("PerLevelStatsMultiplier_Player[11]=1.25"));
    assert!(game_ini_text.contains("PerLevelStatsMultiplier_DinoWild[0]=1.1"));
    assert!(game_ini_text.contains("PerLevelStatsMultiplier_DinoWild[8]=1.2"));
    assert!(game_ini_text.contains("PerLevelStatsMultiplier_DinoTamed[0]=1.35"));
    assert!(game_ini_text.contains("PerLevelStatsMultiplier_DinoTamed[7]=1.8"));
    assert!(game_ini_text.contains("PerLevelStatsMultiplier_DinoTamed[8]=1.4"));
    assert!(game_ini_text.contains(
        "LevelExperienceRampOverrides=(ExperiencePointsForLevel[0]=5,ExperiencePointsForLevel[1]=15,ExperiencePointsForLevel[2]=30)"
    ));
    assert!(game_ini_text.contains("OverridePlayerLevelEngramPoints=8"));
    assert!(game_ini_text.contains("OverridePlayerLevelEngramPoints=12"));
    assert!(game_ini_text.contains("OverridePlayerLevelEngramPoints=16"));
    assert!(game_ini_text.contains("SupplyCrateLootQualityMultiplier=2.5"));
    assert!(game_ini_text.contains("FishingLootQualityMultiplier=1.75"));
    assert!(game_ini_text.contains("bDisableLootCrates=false"));
    assert!(game_ini_text.contains("RandomSupplyCratePoints=true"));
    assert!(game_ini_text.contains("bAllowCustomRecipes=true"));
    assert!(game_ini_text.contains("CustomRecipeEffectivenessMultiplier=1.8"));
    assert!(game_ini_text.contains("CustomRecipeSkillMultiplier=0.65"));
    assert!(game_ini_text.contains("PlayerCharacterHealthRecoveryMultiplier=1.4"));
    assert!(game_ini_text.contains("PlayerCharacterStaminaDrainMultiplier=0.85"));
    assert!(game_ini_text.contains("bLimitTurretsInRange=true"));
    assert!(game_ini_text.contains("bHardLimitTurretsInRange=true"));
    assert!(game_ini_text.contains("LimitTurretsNum=80"));
    assert!(game_ini_text.contains("LimitTurretsRange=9000.0"));
    assert!(game_ini_text.contains("bDisableStructurePlacementCollision=true"));
    assert!(game_ini_text.contains(
        "ConfigOverrideSupplyCrateItems=(SupplyCrateClassString=\"SupplyCrate_Level15_C\",MinItemSets=1,MaxItemSets=1,NumItemSetsPower=1.0,bSetsRandomWithoutReplacement=true,ItemSets=((SetName=\"LanGameBlueDrop\",MinNumItems=1,MaxNumItems=1,NumItemsPower=1.0,SetWeight=1.0,bItemsRandomWithoutReplacement=true,ItemEntries=((EntryWeight=1.0,ItemClassStrings=(\"PrimalItemResource_MetalIngot_C\"),ItemsWeights=(1.0),MinQuantity=100.0,MaxQuantity=150.0,MinQuality=1.0,MaxQuality=1.0,bForceBlueprint=false,ChanceToBeBlueprintOverride=0.0)))))"
    ));
    assert!(game_ini_text.contains(
        "ConfigOverrideItemCraftingCosts=(ItemClassString=\"PrimalItem_WeaponStoneHatchet_C\",BaseCraftingResourceRequirements=((ResourceItemTypeString=\"PrimalItemResource_Thatch_C\",BaseResourceRequirement=10.0,bCraftingRequireExactResourceType=false),(ResourceItemTypeString=\"PrimalItemResource_Wood_C\",BaseResourceRequirement=1.0,bCraftingRequireExactResourceType=false)))"
    ));
    assert!(game_ini_text.contains(
        "ConfigOverrideItemMaxQuantity=(ItemClassString=\"PrimalItemResource_MetalIngot_C\",Quantity=(MaxItemQuantity=500,bIgnoreMultiplier=true))"
    ));
    assert!(game_ini_text.contains(
        "DinoClassDamageMultipliers=(ClassName=\"MegaRex_Character_BP_C\",Multiplier=0.7)"
    ));
    assert!(game_ini_text.contains(
        "DinoClassDamageMultipliers=(ClassName=\"AlphaRaptor_Character_BP_C\",Multiplier=1.4)"
    ));
    assert!(game_ini_text.contains(
        "DinoClassResistanceMultipliers=(ClassName=\"Giga_Character_BP_C\",Multiplier=1.25)"
    ));
    assert!(game_ini_text.contains(
        "TamedDinoClassDamageMultipliers=(ClassName=\"Rex_Character_BP_C\",Multiplier=1.6)"
    ));
    assert!(game_ini_text.contains(
        "TamedDinoClassResistanceMultipliers=(ClassName=\"Stego_Character_BP_C\",Multiplier=0.85)"
    ));
    assert!(game_ini_text.contains("PreventMateBoost=true"));
    assert!(game_ini_text.contains("bUseSingleplayerSettings=true"));
    assert!(game_ini_text.contains("bShowCreativeMode=true"));
    assert!(game_ini_text.contains("bUseDinoLevelUpAnimations=false"));
    assert!(game_ini_text.contains("PreventTransferForClassNames=PrimalItem_WeaponTekSword_C"));
    assert!(game_ini_text.contains("PreventTransferForClassNames=PrimalDinoCharacter_BP_Rex_C"));
    assert!(game_ini_text.contains("MatingIntervalMultiplier=0.5"));
    assert!(game_ini_text.contains("LayEggIntervalMultiplier=0.8"));
    assert!(game_ini_text.contains("MatingSpeedMultiplier=1.5"));
    assert!(game_ini_text.contains("EggHatchSpeedMultiplier=4.0"));
    assert!(game_ini_text.contains("BabyMatureSpeedMultiplier=3.0"));
    assert!(game_ini_text.contains("BabyFoodConsumptionSpeedMultiplier=0.8"));
    assert!(game_ini_text.contains("BabyCuddleIntervalMultiplier=0.25"));
    assert!(game_ini_text.contains("BabyCuddleGracePeriodMultiplier=1.5"));
    assert!(game_ini_text.contains("BabyCuddleLoseImprintQualitySpeedMultiplier=0.5"));
    assert!(game_ini_text.contains("BabyImprintingStatScaleMultiplier=1.2"));
    assert!(game_ini_text.contains("BabyImprintAmountMultiplier=1.5"));
    assert!(game_ini_text.contains("AllowAnyoneBabyImprintCuddle=true"));
    assert!(game_ini_text.contains("DisableImprintDinoBuff=false"));
    assert!(game_ini_text.contains("bAutoUnlockAllEngrams=true"));
    assert!(game_ini_text.contains(
        "EngramEntryAutoUnlocks=(EngramClassName=\"EngramEntry_WoodWall_C\",LevelToAutoUnlock=10)"
    ));
    assert!(game_ini_text.contains(
        "EngramEntryAutoUnlocks=(EngramClassName=\"EngramEntry_StoneWall_C\",LevelToAutoUnlock=20)"
    ));
    assert!(game_ini_text.contains(
        "OverrideNamedEngramEntries=(EngramClassName=\"EngramEntry_TekClaws_C\",EngramHidden=false,EngramPointsCost=0,EngramLevelRequirement=100,RemoveEngramPreReq=true)"
    ));
    assert!(game_ini_text.contains(
        "NPCReplacements=(FromClassName=\"MegaRaptor_Character_BP_C\",ToClassName=\"Raptor_Character_BP_C\")"
    ));
    assert!(game_ini_text.contains(
        "DinoSpawnWeightMultipliers=(DinoNameTag=\"Raptor\",SpawnWeightMultiplier=1.5,OverrideSpawnLimitPercentage=true,SpawnLimitPercentage=0.25)"
    ));
    assert!(game_ini_text.contains(
        "ConfigAddNPCSpawnEntriesContainer=(NPCSpawnEntriesContainerClassString=\"DinoSpawnEntriesBeach_C\",NPCSpawnEntries=((AnEntryName=\"LanGameRaptor\",EntryWeight=0.2,NPCsToSpawnStrings=(\"Raptor_Character_BP_C\"))),NPCSpawnLimits=((NPCClassString=\"Raptor_Character_BP_C\",MaxPercentageOfDesiredNumToAllow=0.2)))"
    ));
    assert!(game_ini_text.contains(
        "ConfigSubtractNPCSpawnEntriesContainer=(NPCSpawnEntriesContainerClassString=\"DinoSpawnEntriesBeach_C\",NPCSpawnEntries=((AnEntryName=\"Dodo\",NPCsToSpawnStrings=(\"Dodo_Character_BP_C\"))))"
    ));
    assert!(game_ini_text.contains(
        "ConfigOverrideNPCSpawnEntriesContainer=(NPCSpawnEntriesContainerClassString=\"DinoSpawnEntriesBeach_C\",NPCSpawnEntries=((AnEntryName=\"LanGameCarno\",EntryWeight=0.1,NPCsToSpawnStrings=(\"Carno_Character_BP_C\"))),NPCSpawnLimits=((NPCClassString=\"Carno_Character_BP_C\",MaxPercentageOfDesiredNumToAllow=0.05)))"
    ));

    assert_eq!(admin_ids_text.trim(), "9999");
    assert!(exclusive_list_text.contains("1111"));
    assert!(exclusive_list_text.contains("2222"));
    assert_eq!(
        fs::read_to_string(live_config_root.join("GameUserSettings.ini")).unwrap(),
        game_user_settings_text
    );
    assert_eq!(
        fs::read_to_string(live_config_root.join("Game.ini")).unwrap(),
        game_ini_text
    );
    assert_eq!(
        fs::read_to_string(live_saved_root.join("AllowedCheaterAccountIDs.txt")).unwrap(),
        admin_ids_text
    );
    assert_eq!(
        fs::read_to_string(live_binary_root.join("PlayersExclusiveJoinList.txt")).unwrap(),
        exclusive_list_text
    );
    assert_eq!(
        fs::read_to_string(live_binary_root.join("PlayersJoinNoCheckList.txt")).unwrap(),
        fs::read_to_string(config_root.join("PlayersJoinNoCheckList.txt")).unwrap()
    );
    assert!(
        instance_private_runtime_root(&created)
            .join("ShooterGame")
            .join("Saved")
            .join(&created.summary.id)
            .join("cluster")
            .exists(),
        "ARK: Survival Ascended should pre-create the live cluster directory when cluster_id is set"
    );
    assert!(
        !config_root.join("launch-arksa.bat").exists(),
        "ARK: Survival Ascended should no longer render a generated launch script"
    );

    cleanup_root(&root);
}
