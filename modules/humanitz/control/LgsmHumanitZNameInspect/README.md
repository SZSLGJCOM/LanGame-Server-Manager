# 当前运行的函数名及字段元数据

这是 Script 只读快照的补充诊断 mod，不替换已运行过的 `LgsmHumanitZInspect`，不改变生产保存。固定 UE4SS `527a483b63b4dd0104fe1ca1a3934a06b87fcfb2` 的 `FName.GetComparisonIndex`、Property.GetAddress 和 GetOffset_Internal 已按上游源及 Git blob SHA 核实；本脚本没有 FName 构造、属性值读取、游戏函数调用、hook 或保存调用。

部署 `<runtime>/HumanitZServer/Binaries/Win64/ue4ss/Mods/LgsmHumanitZNameInspect/Scripts/main.lua` 及同一 mod 的空 `enabled.txt`。复用前两阶段的固定 core、诊断 winmm 和 `LGSM_HUMANITZ_INSPECT_RUN`，以及同一物理副本的 `langame_humanitz_inspect` 目录。不要部署原始包或复用前一个进程的对象地址。

通过 `names-request.txt` 发三个严格 ASCII/LF 行：

```text
schema=1
run=<本次启动32lowerhex>
nonce=<本次新32lowerhex>
```

主线程只读扫描并写 `names-<nonce>.json`。最多八个不同 nonce、524288 个对象、65536 个函数、每个声明最多 8192 字段、全局 65536 字段，输出最多 32 MiB。每个请求生成新文件保留；超出枚举界限标 complete=false。

根字段闭合为 schema/module/stage/run/nonce/complete/visited/property_count/functions/objects/classes，stage 为 name_metadata_only。所有 UFunction 都记录 address、完整函数名、owner、flags 和 ComparisonIndex；仅 RCON、GameModeSave、GameInstanceWithSettings 三个实际 BP 家族的函数参数/局部变量及类字段记录描述符地址、名称和 offset，不读取这些字段的值。properties_requested 区分是否要求该函数的字段列表，properties_complete 表明该请求列表是否完整。

同一进程里随后用既有 `LgsmHumanitZScriptInspect` 和已验证原生 helper 抽取 Script。目标必须从这次 names JSON 的地址重新选择：优先完整覆盖以下三个 owner，再加原生 HMZRcon:BPDataReceived 与两个 Engine SaveGameToSlot 入口。上一运行相应 BP owner 合计 446 个函数，在原生 512 目标上限内；本次若超过上限要明确分批，不能截断这些家族。

```text
BP_HMZRCon.BP_HMZRCon_C
BP_GameModeSave.BP_GameModeSave_C
BP_GameInstanceWithSettings.BP_GameInstanceWithSettings_C
```

关键是必须包含三个 `ExecuteUbergraph_*`，尤其前次漏掉的 GameInstanceWithSettings ubergraph。函数名 ComparisonIndex、描述符指针、函数地址和 Script 字节必须来自同一个 run/PID/FILETIME 对应的实时进程；不能用前一次函数名表解释后一次字节码。新原生 Script 输出继续由 `decode_scripts.py` 严格校验本次 run、nonce、PID 和精确 FILETIME。

已有本机只读快照证明 Script 布局和 RCON/GM stub 到 ubergraph 的实际入口；GM graph 内存在五处原生异步 SaveGameToSlot 调用和六处同步调用。这还不是保存闭环结论：需要本次函数名/字段映射、分支和 callback/latent 根归属，以及每个 native writer 的终端结果。这里只补取缺失因果证据，不执行实际保存。

固定加载器的 `GetOuter()` 明确构造普通 UObject 包装，不能调用 UStruct 的 ForEachProperty。探针通过 StaticFindObject 查找这个已经存在的 owner，要求相同地址、完整名称和 IsAnyClass，然后使用该 API 自动构造的 UClass 包装枚举类字段。没有加载新资产、创建对象或猜测内存 offset。该边界已按固定源核实，并由普通 outer 不含 ForEachProperty 的红→绿反例覆盖。

失败不会只留下 generic 文案：日志包含固定 failure_stage、静态 issue 及 Lua error_kind；另写 `name-error-<nonce>.json`，绑定本次 run/nonce，stage 为 name_metadata_failed，记录 failure_stage/issue/error_kind/visited/function_count/property_count。不会输出原始错误字符串、路径、对象内容或玩家信息。失败诊断输出本身不可用时，静态日志仍保留原始失败分类。

阶段覆盖 game_thread、function_class、object_enumeration、object_validity、function_type/name/outer/owner、get_fname、comparison_index、function/class_fields_enumeration、field_offset/name/descriptor、owner_class_lookup、function_address/flags、object_metadata 及 output_size/lookup/open/write/close/publish。检查条件和数量边界没有因为诊断失败而放宽。

组件 65 个断言覆盖主线程、原生/Blueprint 函数 ComparisonIndex、超过旧 128 字段上限的完整记录、正确的 typed owner、类字段、错误身份、额外字段、nonce 幂等、八次预算、截断，以及九类 API/数据/输出失败的精确分类及秘密不泄漏。UE API 为边界夹具；没有将其称为真实专服集成。
