export interface SoulmaskGroupSpec {
  id: string;
  title: string;
  description: string;
  layoutClass: string;
  keys: string[];
}

export const SOULMASK_GROUP_SPECS: Record<string, SoulmaskGroupSpec[]> = {
  "access": [
    {
      id: "passwords",
      title: "Administrator password",
      description: "Administrator credentials for the dedicated host.",
      layoutClass: "soulmask-passwords",
      keys: [
        "admin_password",
      ]
    }
  ],
  "world": [
    {
      id: "mode",
      title: "Rule mode",
      description: "Choose PvE or PvP rules for the world.",
      layoutClass: "soulmask-mode",
      keys: ["pvp_mode_arg"]
    },
    {
      id: "persistence",
      title: "Persistence",
      description: "Auto-save and backup cadence in seconds.",
      layoutClass: "soulmask-persistence",
      keys: [
        "save_interval_seconds",
        "backup_interval_seconds"
      ]
    }
  ],
  "advanced": [
    {
      id: "launch",
      title: "Launch Overrides",
      description: "One full launch argument per line appended to startup.",
      layoutClass: "soulmask-launch",
      keys: [
        "mod_workshop_ids",
        "extra_launch_args"
      ]
    }
  ],
  "xishu_general": [
    {
      id: "general",
      title: "General Gameplay",
      description: "Core toggles, time, followers, permissions, and miscellaneous world behavior.",
      layoutClass: "soulmask-general",
      keys: [
        "xishu_add_ren_ke_du_ratio",
        "xishu_bin_si_kai_guan",
        "xishu_kuai_su_dao_da_on_zu_dang_kai_guan",
        "xishu_jian_zhu_fu_lan_kai_guan",
        "xishu_game_world_day_time_portion",
        "xishu_game_world_time_power",
        "xishu_hu_x_iang_shang_hai_kai_guan",
        "xishu_wan_mei_chong_su",
        "xishu_fu_huo_move_si_wang_bao_kai_guan",
        "xishu_ru_qin_kai_guan",
        "xishu_jian_dui_ru_qin_kai_guan",
        "xishu_shua_xin_npc_kai_guan",
        "xishu_chongsu_ratio",
        "xishu_xiu_mian_distance",
        "xishu_huan_xing_distance",
        "xishu_gong_hui_max_zhao_mu_count",
        "xishu_ge_ren_max_zhao_mu_count",
        "xishu_ge_ren_max_zhao_mu_count_two",
        "xishu_ge_ren_max_zhao_mu_count_three",
        "xishu_xin_qing_zeng_zhang",
        "xishu_xin_qing_jian_shao",
        "xishu_xi_shu_wei_ling",
        "xishu_yun_xu_other_da_kai_gong_zuo_tai",
        "xishu_yun_xu_other_da_kai_xiang_zi",
        "xishu_pve_only_tong_gui_shu_can_open_kai_guan",
        "xishu_gong_hui_max_dong_wu_count",
        "xishu_ge_ren_max_dong_wu_count",
        "xishu_panpa_kai_guan",
        "xishu_bao_xiang_diao_luo_deng_ji",
        "xishu_kai_qi_kua_fu",
        "xishu_hu_dong_exclude_between_camera_character",
        "xishu_wu_li_you_hua_dist",
        "xishu_movement_you_hua",
        "xishu_xiu_mian_offline_days",
        "xishu_wu_li_you_hua_kai_guan",
        "xishu_tiao_wu_leng_que_time",
        "xishu_xin_xi_lu_ru",
        "xishu_max_fu_zhong_ratio",
        "xishu_role_bag_capacity",
        "xishu_ge_ren_biao_ji_max_count",
        "xishu_gong_hui_biao_ji_max_count",
        "xishu_make_use_around_rong_qi_kai_guan",
        "xishu_gong_hui_max_member",
        "xishu_zhao_huan_dis_ratio",
        "xishu_zu_ren_fu_zhi",
        "xishu_conver_props_speed_ratio",
        "xishu_max_convert_count",
        "xishu_war_kai_guan",
        "xishu_tribal_exploration_kai_guan",
        "xishu_ruins_exploration_kai_guan",
        "xishu_tribal_transport_switch",
        "xishu_special_boss_switch",
        "xishu_relic_chest_event_switch",
        "xishu_boss_death_event_switch",
        "xishu_kurma_fu_zhong_ratio",
        "xishu_gong_hui_max_spec_dong_wu_count",
        "xishu_ge_ren_max_spec_dong_wu_count",
        "xishu_protect_jian_zhu_in_ying_huo_switch",
        "xishu_mask_repair_upgrade_switch",
        "xishu_is_open_guide_task",
        "xishu_mental_recovery_rate",
        "xishu_is_play_boss_appearance_sequence",
        "xishu_player_death_cant_drop_item_kai_guan",
        "xishu_animal_follower_max_count",
        "xishu_draw_debug_dungeon",
        "xishu_ban_glider",
        "xishu_jian_zhu_mirage_kai_guan",
        "xishu_max_conveyor_count",
        "xishu_max_dong_li_kuang_chang_count",
        "xishu_ping_tai_build_range_limit",
        "xishu_ge_ren_max_raft_space_count",
        "xishu_gong_hui_max_raft_space_count",
        "xishu_ge_ren_max_spec_raft_space_count",
        "xishu_gong_hui_max_spec_raft_space_count",
        "xishu_max_di_ci_count",
        "xishu_ping_tai_affect_navigation",
        "xishu_ji_qi_chu_zhan_kai_guan",
        "xishu_zu_ren_direct_cun_qu",
        "xishu_bag_rep_optimize_switch",
        "xishu_jin_jian_qu_kai_guan",
        "xishu_crew_count_ratio",
        "xishu_ignore_enemy_jian_zhu_in_self_ying_huo",
        "xishu_ship_blueprint_build_consume_switch",
        "xishu_chest_drop_equipment_max_quality_switch",
        "xishu_main_gun_use_time_cd",
        "xishu_restart_game_force_spawn_monster_switch"
      ]
    }
  ],
  "xishu_progression": [
    {
      id: "progression",
      title: "Progression",
      description: "Experience, leveling, attributes, training, and skill progression.",
      layoutClass: "soulmask-progression",
      keys: [
        "xishu_exp_ratio",
        "xishu_cheng_zhang_exp_ratio",
        "xishu_mj_exp_ratio",
        "xishu_shu_lian_du_exp_ratio",
        "xishu_cai_ji_exp_ratio",
        "xishu_zhi_zuo_exp_ratio",
        "xishu_sha_guai_exp_ratio",
        "xishu_qi_ta_exp_ratio",
        "xishu_bei_dong_yi_ji_shu_xing_ratio",
        "xishu_zhu_dong_yi_ji_shu_xing_ratio",
        "xishu_er_ji_shu_xing_ratio",
        "xishu_ma_ren_bei_dong_yi_ji_shu_xing_ratio",
        "xishu_ma_ren_zhu_dong_yi_ji_shu_xing_ratio",
        "xishu_ma_ren_er_ji_shu_xing_ratio",
        "xishu_dong_wu_bei_dong_yi_ji_shu_xing_ratio",
        "xishu_dong_wu_zhu_dong_yi_ji_shu_xing_ratio",
        "xishu_dong_wu_er_ji_shu_xing_ratio",
        "xishu_hui_fu_chu_shi_body_data",
        "xishu_sha_guai_exp_share_ratio",
        "xishu_max_level",
        "xishu_training_exp_ratio",
        "xishu_chu_zhan_zu_ren_sha_guai_exp_share_ratio",
        "xishu_other_sha_guai_exp_share_ratio",
        "xishu_cur_prof_init_ratio"
      ]
    }
  ],
  "xishu_yields": [
    {
      id: "yields",
      title: "Yields & Crafting",
      description: "Harvest, drops, crops, animal production, crafting speed, and equipment drop correction.",
      layoutClass: "soulmask-yields",
      keys: [
        "xishu_zuo_wu_drop_ratio",
        "xishu_zuo_wu_sheng_zhang_ratio",
        "xishu_zhi_zuo_time_ratio",
        "xishu_bao_xiang_drop_ratio",
        "xishu_cai_ji_diao_luo_ratio",
        "xishu_fa_mu_diao_luo_ratio",
        "xishu_cai_kuang_diao_luo_ratio",
        "xishu_dong_wu_shi_ti_diao_luo_ratio",
        "xishu_dong_wu_shi_ti_zhong_yao_diao_luo_ratio",
        "xishu_cai_ji_sheng_chan_jian_zhu_diao_luo_ratio",
        "xishu_pu_tong_ren_diao_luo_ratio",
        "xishu_jing_ying_ren_diao_luo_ratio",
        "xishu_boss_ren_diao_luo_ratio",
        "xishu_cai_ji_damage_ratio",
        "xishu_zi_yuan_sheng_ming_ratio",
        "xishu_dong_wu_sheng_zhang_ratio",
        "xishu_fan_zhi_jian_ge_ratio",
        "xishu_dong_wu_sheng_chan_jian_ge_ratio",
        "xishu_dong_wu_chan_chu_ratio",
        "xishu_zuo_wu_xiao_hui_ratio",
        "xishu_fu_hua_speed",
        "xishu_te_shu_dao_ju_drop_xi_shu_jia_cheng_kai_guan",
        "xishu_normal_equip_drop_ratio_correction",
        "xishu_elite_equip_drop_ratio_correction",
        "xishu_boss_equip_drop_ratio_correction",
        "xishu_normal_equip_durability_correction",
        "xishu_elite_equip_durability_correction",
        "xishu_boss_equip_durability_correction"
      ]
    }
  ],
  "xishu_building": [
    {
      id: "building",
      title: "Building",
      description: "Decay, repair, portals, campfires, construction limits, and building interaction rules.",
      layoutClass: "soulmask-building",
      keys: [
        "xishu_jian_zhu_fu_lan_mul",
        "xishu_jian_zhu_xiu_li_mul",
        "xishu_jian_zhu_chuan_song_men_plus_kai_guan",
        "xishu_ye_sheng_hit_jian_zhu_shang_hai_ratio",
        "xishu_wan_jia_hit_jian_zhu_shang_hai_ratio",
        "xishu_kai_qi_jian_zhu_hui_xue_building",
        "xishu_ying_huo_ran_shao_su_du_ratio",
        "xishu_max_gen_ren_ying_huo_number",
        "xishu_max_gong_hui_ying_huo_number",
        "xishu_max_chuan_song_men_number",
        "xishu_jian_zhu_gao_du_limit",
        "xishu_jian_zhu_be_damage_limit",
        "xishu_jian_zhu_around_num_limit",
        "xishu_trans_door_interwork_kai_guan",
        "xishu_max_xiu_mian_cang_count",
        "xishu_ping_tai_jian_zhu_num_limit",
        "xishu_max_ping_tai_jian_zhu_num_mul",
        "xishu_open_esc_menu_inf_jian_zao",
        "xishu_new_ying_huo_time_len_mul",
        "xishu_enable_float_foundation",
        "xishu_max_float_foundation_num",
        "xishu_max_cai_ji_chang_limit_count",
        "xishu_max_fa_mu_chang_limit_count",
        "xishu_max_wa_jue_chang_limit_count"
      ]
    }
  ],
  "xishu_resources": [
    {
      id: "resources",
      title: "Resource Respawn",
      description: "Vegetation and resource respawn radii.",
      layoutClass: "soulmask-resources",
      keys: [
        "xishu_zhi_bei_chong_sheng_ratio",
        "xishu_wan_jia_zi_yuan_jin_shua_ban_jing",
        "xishu_jian_zhu_zi_yuan_jin_shua_ban_jing"
      ]
    }
  ],
  "xishu_combat": [
    {
      id: "combat",
      title: "Combat",
      description: "Damage, recovery, quality, PvP damage, boss, dungeon, and tenacity tuning.",
      layoutClass: "soulmask-combat",
      keys: [
        "xishu_damage_ye_sheng_ratio",
        "xishu_be_damage_by_ye_sheng_ratio",
        "xishu_sheng_ming_hui_fu_ratio",
        "xishu_ti_li_hui_fu_ratio",
        "xishu_qi_xi_hui_fu_ratio",
        "xishu_dong_wu_damage_ratio",
        "xishu_dong_wu_jian_shang_ratio",
        "xishu_ma_ren_damage_ratio",
        "xishu_man_ren_jian_shang_ratio",
        "xishu_jia_si_hui_fu_ratio",
        "xishu_gong_ji_jian_zhu_damage_ratio",
        "xishu_dong_wu_pin_zhi_ratio",
        "xishu_man_ren_pin_zhi_ratio",
        "xishu_pvp_shang_hai_ratio_without_p2_p_you_fang",
        "xishu_pvp_shang_hai_ratio_jin_zhan",
        "xishu_pvp_shang_hai_ratio_yuan_cheng",
        "xishu_wan_jia_bei_xiao_ren_ratio",
        "xishu_wan_jia_bei_xiao_ti_ratio",
        "xishu_pvp_shang_hai_ratio_player_to_player_di_fang",
        "xishu_pvp_ga_pvp_damage_ratio",
        "xishu_player_you_fang_shang_hai_kai_guan",
        "xishu_you_fang_shang_hai_kai_guan",
        "xishu_pvp_shang_hai_ratio_player_to_player_you_fang",
        "xishu_dynamic_boss_stats",
        "xishu_suo_ding_kai_guan",
        "xishu_rolling_invincible_time_ratio",
        "xishu_physical_recovery_interval_rate",
        "xishu_dungeon_reborn",
        "xishu_man_ren_ti_li_damage_ratio",
        "xishu_man_ren_tenacity_damage_ratio",
        "xishu_man_ren_boss_ti_li_damage_ratio",
        "xishu_man_ren_boss_tenacity_damage_ratio",
        "xishu_dong_wu_ti_li_damage_ratio",
        "xishu_dong_wu_tenacity_damage_ratio",
        "xishu_dong_wu_boss_ti_li_damage_ratio",
        "xishu_dong_wu_boss_tenacity_damage_ratio",
        "xishu_player_sweep_range_scale",
        "xishu_rebound_difficulty",
        "xishu_release_control_status_cd_ratio"
      ]
    }
  ],
  "xishu_survival": [
    {
      id: "survival",
      title: "Survival Upkeep",
      description: "Durability, food, water, breath, fuel, spoilage, and repair consumption.",
      layoutClass: "soulmask-survival",
      keys: [
        "xishu_nai_jiu_xi_shu",
        "xishu_shi_wu_xiao_hao_ratio",
        "xishu_shui_xiao_hao_ratio",
        "xishu_qi_xi_xiao_hao_ratio",
        "xishu_ran_liao_xiao_hao_ratio",
        "xishu_dong_wu_xiao_hao_shi_wu_ratio",
        "xishu_dong_wu_xiao_hao_shui_ratio",
        "xishu_zuo_wu_fei_liao_xiao_hao_ratio",
        "xishu_zuo_wu_shui_xiao_hao_ratio",
        "xishu_wu_pin_fu_huai_ratio",
        "xishu_wu_pin_xiao_hui_time",
        "xishu_xiu_li_xu_yao_cai_liao_ratio",
        "xishu_xiu_li_jiang_nai_jiu_shang_xian_ratio",
        "xishu_jing_shen_no_xiao_hao"
      ]
    }
  ],
  "xishu_invasions": [
    {
      id: "invasions",
      title: "Invasions",
      description: "Heat, random invasions, invasion wave size, timing, and rewards.",
      layoutClass: "soulmask-invasions",
      keys: [
        "xishu_re_du_xi_shu",
        "xishu_sui_ji_ru_qin_kai_guan",
        "xishu_ru_qin_gui_mo_xi_shu",
        "xishu_ru_qin_qiang_du_xi_shu",
        "xishu_ru_qin_guai_count_min",
        "xishu_ru_qin_guai_count_max",
        "xishu_ru_qin_per_bo_guai_min",
        "xishu_ru_qin_per_bo_guai_max",
        "xishu_ru_qin_guai_level_xi_shu",
        "xishu_tan_cha_minute_limit",
        "xishu_jin_gong_minute_limit",
        "xishu_leng_que_minute_limit",
        "xishu_ru_qin_max_chang_ci_count",
        "xishu_ru_qin_begin_hour",
        "xishu_ru_qin_end_hour",
        "xishu_ru_qin_shao_cheng_xi_shu",
        "xishu_ru_qin_tu_sha_xi_shu",
        "xishu_manage_mode_ru_qin",
        "xishu_manage_mode_ru_qin_count_down_time_ratio",
        "xishu_ru_qin_succeed_prize_times"
      ]
    }
  ],
  "xishu_pvp_schedule": [
    {
      id: "pvp_schedule",
      title: "PvP Schedule & Level Caps",
      description: "Regional PvP windows and open-server awareness level caps.",
      layoutClass: "soulmask-pvp-schedule",
      keys: [
        "xishu_pvp_time_asia_work_start_time",
        "xishu_pvp_time_asia_work_end_time",
        "xishu_pvp_time_asia_no_work_start_time",
        "xishu_pvp_time_asia_no_work_end_time",
        "xishu_pvp_time_america_work_start_time",
        "xishu_pvp_time_america_work_end_time",
        "xishu_pvp_time_america_no_work_start_time",
        "xishu_pvp_time_america_no_work_end_time",
        "xishu_pvp_time_europe_work_start_time",
        "xishu_pvp_time_europe_work_end_time",
        "xishu_pvp_time_europe_no_work_start_time",
        "xishu_pvp_time_europe_no_work_end_time",
        "xishu_initial_default_awareness_level",
        "xishu_first_day_max_awareness_level",
        "xishu_second_day_max_awareness_level",
        "xishu_third_day_max_awareness_level",
        "xishu_fourth_day_max_awareness_level",
        "xishu_fifth_day_max_awareness_level",
        "xishu_sixth_day_max_awareness_level",
        "xishu_seventh_day_max_awareness_level",
        "xishu_eighth_day_max_awareness_level",
        "xishu_ninth_day_max_awareness_level",
        "xishu_tenth_day_max_awareness_level"
      ]
    }
  ],
  "xishu_ai_followers": [
    {
      id: "ai_followers",
      title: "AI & Followers",
      description: "AI difficulty and active follower counts.",
      layoutClass: "soulmask-ai-followers",
      keys: [
        "xishu_ai_deng_ji",
        "xishu_man_ren_chu_zhan_count",
        "xishu_dong_wu_chu_zhan_count"
      ]
    }
  ],
  "xishu_battlefield": [
    {
      id: "battlefield",
      title: "Battlefield Windows",
      description: "Regional battlefield start and end windows.",
      layoutClass: "soulmask-battlefield",
      keys: [
        "xishu_asia_war_time_start",
        "xishu_asia_war_time_end",
        "xishu_europe_war_time_start",
        "xishu_europe_war_time_end",
        "xishu_america_war_time_start",
        "xishu_america_war_time_end"
      ]
    }
  ],
  "xishu_events": [
    {
      id: "events",
      title: "Server Events",
      description: "Global event region, timing, trigger probability, and open-day gates.",
      layoutClass: "soulmask-events",
      keys: [
        "xishu_special_event_config_switch",
        "xishu_special_event_game_dist",
        "xishu_special_event_asia_start_time",
        "xishu_special_event_asia_end_time",
        "xishu_special_event_europe_start_time",
        "xishu_special_event_europe_end_time",
        "xishu_special_event_america_start_time",
        "xishu_special_event_america_end_time",
        "xishu_special_event_trigger_interval",
        "xishu_special_event_trigger_percent",
        "xishu_special_event_trigget_limit_num",
        "xishu_special_event_server_open_day"
      ]
    }
  ],
};
