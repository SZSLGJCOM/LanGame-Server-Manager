; Tauri /UPDATE replaces its uninstall file list without removing retired files.
; Keep retirement identities here: exact bundled paths and original LF/CRLF
; SHA256 values. Unknown or edited files are not installer-owned cleanup targets.
!ifndef LGSM_RETIRED_RESOURCES
!define LGSM_RETIRED_RESOURCES

!macro LGSM_RETIRED_RESOURCE_FUNCTION prefix
Function ${prefix}LGSMRemoveRetiredResource
  ; $0: absolute file, $1/$2: accepted hashes. Caller saves the NSIS registers.
  StrCpy $3 -1
  StrCpy $5 0
  StrCpy $6 0
  StrCpy $7 0
  StrCpy $R9 0
  ; Check the file and every ancestor, including custom installation roots.
  StrCpy $R0 $0
  lgsm_resource_path:
    System::Call 'kernel32::GetFileAttributesW(w R0) i.R1 ?e'
    Pop $R2
    IntCmp $R1 -1 lgsm_resource_missing
    IntOp $R1 $R1 & 0x400
    IntCmp $R1 0 0 lgsm_resource_preserved lgsm_resource_preserved
    ${GetParent} "$R0" $R1
    StrCmp $R1 "" lgsm_resource_open
    StrCmp $R1 $R0 lgsm_resource_open
    StrCpy $R0 $R1
    Goto lgsm_resource_path
  lgsm_resource_missing:
    IntCmp $R2 2 lgsm_resource_done
    IntCmp $R2 3 lgsm_resource_done lgsm_resource_failed lgsm_resource_failed

  lgsm_resource_open:
    ; Deny writers/deletion while checking bounded retired package resources.
    System::Call 'kernel32::CreateFileW(w r0, i 0x80010000, i 1, p 0, i 3, i 0x00200000, p 0) p.r3'
    IntCmp $3 -1 lgsm_resource_failed
    System::Call 'kernel32::GetFinalPathNameByHandleW(p r3, w.R0, i ${NSIS_MAX_STRLEN}, i 0) i.R1'
    IntCmp $R1 0 lgsm_resource_failed
    IntCmp $R1 ${NSIS_MAX_STRLEN} lgsm_resource_preserved 0 lgsm_resource_preserved
    StrCpy $R2 "\\?\$0"
    StrCmp $R0 $R2 0 lgsm_resource_preserved
    System::Call 'kernel32::GetFileSize(p r3, *i.R2) i.r4 ?e'
    Pop $R0
    ${If} $4 = -1
    ${AndIf} $R0 <> 0
      Goto lgsm_resource_failed
    ${EndIf}
    IntCmp $R2 0 0 lgsm_resource_preserved lgsm_resource_preserved
    IntCmpU $4 65536 lgsm_resource_preserved 0 lgsm_resource_preserved
    IntCmp $4 0 lgsm_resource_preserved lgsm_resource_failed
    System::Alloc $4
    Pop $5
    IntCmp $5 0 lgsm_resource_failed
    System::Call 'kernel32::ReadFile(p r3, p r5, i r4, *i.R0, p 0) i.r8'
    IntCmp $8 0 lgsm_resource_failed
    IntCmp $R0 $4 0 lgsm_resource_failed lgsm_resource_failed
    System::Alloc 32
    Pop $6
    IntCmp $6 0 lgsm_resource_failed
    System::Call 'bcrypt::BCryptOpenAlgorithmProvider(*p.r7, w "SHA256", p 0, i 0) i.r8'
    IntCmp $8 0 0 lgsm_resource_failed lgsm_resource_failed
    System::Call 'bcrypt::BCryptHash(p r7, p 0, i 0, p r5, i r4, p r6, i 32) i.r8'
    IntCmp $8 0 0 lgsm_resource_failed lgsm_resource_failed
    System::Call 'crypt32::CryptBinaryToStringW(p r6, i 32, i 0x4000000c, w.r9, *i 65) i.r8'
    IntCmp $8 0 lgsm_resource_failed
    StrCmp $9 $1 lgsm_resource_remove
    StrCmp $9 $2 0 lgsm_resource_preserved
  lgsm_resource_remove:
    ; Delete through the checked handle, without a close/reopen path race.
    System::Call 'kernel32::SetFileInformationByHandle(p r3, i 4, *i 1, i 4) i.r8'
    IntCmp $8 0 lgsm_resource_failed
    System::Call 'kernel32::CloseHandle(p r3)'
    StrCpy $3 -1
    DetailPrint "Removed retired package resource: $0"
    ${GetParent} "$0" $R0
    ; Remove only an empty directory whose ordinary ancestors were checked.
    RMDir "$R0"
    Goto lgsm_resource_done
  lgsm_resource_preserved:
    DetailPrint "Preserved non-package resource: $0"
    Goto lgsm_resource_done
  lgsm_resource_failed:
    StrCpy $R9 1
  lgsm_resource_done:
    IntCmp $3 -1 +2
      System::Call 'kernel32::CloseHandle(p r3)'
    IntCmp $7 0 +2
      System::Call 'bcrypt::BCryptCloseAlgorithmProvider(p r7, i 0)'
    System::Free $6
    System::Free $5
    Push $R9
FunctionEnd
!macroend

!insertmacro LGSM_RETIRED_RESOURCE_FUNCTION ""
!insertmacro LGSM_RETIRED_RESOURCE_FUNCTION "un."

!macro LGSM_REMOVE_RETIRED_RESOURCE prefix relative hashLF hashCRLF
  System::Store S
  StrCpy $0 "$INSTDIR\${relative}"
  StrCpy $1 "${hashLF}"
  StrCpy $2 "${hashCRLF}"
  Call ${prefix}LGSMRemoveRetiredResource
  Pop $R0
  ${If} $R0 <> 0
    DetailPrint "$(lgsmResourceCleanupFailed): $0"
    System::Store L
    SetErrorLevel 11
    Abort "$(lgsmResourceCleanupFailed)"
  ${EndIf}
  System::Store L
!macroend

!macro LGSM_REMOVE_RETIRED_RESOURCES prefix
  ; Retired native save observers and unused diagnostics from prior packages.
  !insertmacro LGSM_REMOVE_RETIRED_RESOURCE "${prefix}" "modules\humanitz\control\Build-Components.ps1" "773f06fcf988be92d6c6376f32e70bc0c413de714fed1cc702dbdd58daba7824" "f3dbed6e9f82dbfbe9061ff1e48e7a92838deafb82ac8aab22849b0c38be17a4"
  !insertmacro LGSM_REMOVE_RETIRED_RESOURCE "${prefix}" "modules\humanitz\control\LgsmHumanitZInspect\Scripts\main.lua" "8427b2344d7cd284c6d0a3381433aed76940f27b63556ede2daec426d0e39462" "a27129e96303d23834497803b936843e772469140594f7618326665494168f51"
  !insertmacro LGSM_REMOVE_RETIRED_RESOURCE "${prefix}" "modules\humanitz\control\LgsmHumanitZInspect\tests.lua" "b19af856566f318d6e0af4e9028b4c994f5e4f9c277fd6bfb9cb9851ccf0524c" "1cccd477e516f5700feedb30b2ddf7585951b264110c5a53d8081d4b7c849b19"
  !insertmacro LGSM_REMOVE_RETIRED_RESOURCE "${prefix}" "modules\humanitz\control\LgsmHumanitZNameInspect\README.md" "f1c6a854562a613a7a5e967c84a034162d803d50392fed1894282659c9168ab7" "39c883f7e536ff132a62ab238d3140a12873bd4e3a6b37ff42f1229e65ed9351"
  !insertmacro LGSM_REMOVE_RETIRED_RESOURCE "${prefix}" "modules\humanitz\control\LgsmHumanitZNameInspect\Scripts\main.lua" "abfcd7eef5ecd2c3e11277005e43de522151557ec909a4b081a84c144306b0bf" "bb7a55c09ffa3f06aa38346ce516d5c108a677a751beb7bc0d764c1ec9864e14"
  !insertmacro LGSM_REMOVE_RETIRED_RESOURCE "${prefix}" "modules\humanitz\control\LgsmHumanitZNameInspect\tests.lua" "c784b5610d6bc03b6ca3806d2189302bd7d9a607d16b2f07c224286a57e2dada" "229ba15c4df74491d7fee5b4e297bb1baba3a4f55b9ef17fef93fb11a03cb09f"
  !insertmacro LGSM_REMOVE_RETIRED_RESOURCE "${prefix}" "modules\humanitz\control\decode_scripts.py" "13303f859dff0f140432d93e4260109509fdce52e181b48a17c20f268ee2508e" "9d4df40dceb2dac9f89c7232e7c6f421139a705b8d0bddf6bf307fbc0086dd6e"
  !insertmacro LGSM_REMOVE_RETIRED_RESOURCE "${prefix}" "modules\humanitz\control\script_bridge.cpp" "530cc621e51e41dbc803268fc5738905948c962d9bf1a48c887262bbfa12c620" "9b3de7b24f7f6db22ee43ad6f1be3f9986456768f2a09d4cc462bd293afaa231"
  !insertmacro LGSM_REMOVE_RETIRED_RESOURCE "${prefix}" "modules\humanitz\control\script_snapshot.hpp" "c322b81ff0bfc174bdbc2a22d0f1d3f541622974c6626c4ae9306adfbd1126ed" "9b7dc88abf9d8c4ede62936f1107f206f6ca5d326a913ac228d1543438e0afc4"
  !insertmacro LGSM_REMOVE_RETIRED_RESOURCE "${prefix}" "modules\humanitz\control\script_snapshot_tests.cpp" "1cd7b60b1df57abd886734af4dd950c69ead77bdf3bc951d4d972fb061b0075d" "7dc3f34b671b8a586a79efd652efa1e8084a5a67fa5a91b290efb361e022ab46"
  !insertmacro LGSM_REMOVE_RETIRED_RESOURCE "${prefix}" "modules\humanitz\control\script_trigger.lua" "d85f7ef39a4dda4b7b3477a6818d02a8c3c9548dcaff088b6171d3fcc42b8f84" "120cd75317c1fae12d70f13d0873c91fe16e41d1f7d715d82147b7de5a9741e2"
  !insertmacro LGSM_REMOVE_RETIRED_RESOURCE "${prefix}" "modules\humanitz\control\script_trigger_tests.lua" "fa1a9532c6f1fb00e581ffffe5a6e8537ecacc5f25fffe9843c84320416779d1" "78e8de277c0193b5fc4b053faeb9098c5ef857dff21e36d13f18550ab3817634"
  !insertmacro LGSM_REMOVE_RETIRED_RESOURCE "${prefix}" "modules\nightingale\control\disk_snapshot.hpp" "9c008054972e27289173ad5ad90783ea3b5de49b801087c0ccf67bfa441112eb" "fd23766af788aa6509767a4a3225e0ee268d375ed5d3d418e0d26b5909651c2c"
  !insertmacro LGSM_REMOVE_RETIRED_RESOURCE "${prefix}" "modules\nightingale\control\tests\policy_fixture.lua" "2c9fa24a5ccfaa4203796a4ebad7d0482e895fe5e0e31139ef49d448a261d901" "6dc07b31d873a4f2d1769dd5e5c4900a59084e8a226bc2fe26ece7b9568969c1"
  !insertmacro LGSM_REMOVE_RETIRED_RESOURCE "${prefix}" "modules\nightingale\control\tests\snapshot_tests.cpp" "88670890f0fd8fb2ae8b6bfef9f8193f341c265febc67e01031bed343cb3f913" "dacc2725323a0807e1ea0f6c12ffe900282d0b19f01a10aaa8b5cc2b88307ac7"
  !insertmacro LGSM_REMOVE_RETIRED_RESOURCE "${prefix}" "modules\returntomoria\control\bridge.cpp" "39a81fef7f4671cfea445903f4e4424233a4bd52f761318df652e35715991d49" "da17f76cafd43a35c3f2a40a912c374c9f249f7db53ef5e132ee5531cdebe493"
  !insertmacro LGSM_REMOVE_RETIRED_RESOURCE "${prefix}" "modules\returntomoria\control\tests\bridge_tests.cpp" "d1c384afa2381640f5793fbb93dd620f644776884964e651fee04044b15260b0" "32ed474e06439af663bad27d8f2f45509df71132b5dcbdc6cf72c8025890f739"
  !insertmacro LGSM_REMOVE_RETIRED_RESOURCE "${prefix}" "modules\romestead\templates\start-romestead.bat.hbs" "febb34e9c83d6ee13c4876d6b718a0447d6b744c4133401aed94e5ef1b21a7d0" "dec610b49b2bdc0b71edd8f2ccb42a342ad480fe3a945e8c0581850b36f0cc8a"
  !insertmacro LGSM_REMOVE_RETIRED_RESOURCE "${prefix}" "modules\runescapedragonwilds\control\Build-Components.ps1" "75abb89bae09406e7122cc7fcaf14659a6f64d1a9a66a0867e60adb6ed63a1ab" "ab16f668b3c24a8fbeb5073f9317952939c20fa29dd5ea537d8b1bef4fcefad1"
  !insertmacro LGSM_REMOVE_RETIRED_RESOURCE "${prefix}" "modules\runescapedragonwilds\control\save_bridge.cpp" "e3d25553f732da0f65835473986f5431101c9ae33f173d6fabef7194b08396f4" "3ce155d4105d48757b0d22e34e3192ad44242c83a68665568a139446ab35356d"
  !insertmacro LGSM_REMOVE_RETIRED_RESOURCE "${prefix}" "modules\runescapedragonwilds\control\save_completion.hpp" "ce6ba40c816d951567d576624b803a0ec36aa883f81bc7785f8c5ae295c884bf" "a1f97f301bd3653fe198d642e602594a8d1acb8205b3bb4c0fb471f3915bf868"
  !insertmacro LGSM_REMOVE_RETIRED_RESOURCE "${prefix}" "modules\runescapedragonwilds\control\save_files.cpp" "49680b9a94f43c79c6ffcb19920fbb80e41a4c1dc65cc601f9a4215d5ae8a1b3" "51adcb94e98a9d6126ac51f71ac54d4169483051a57de01cf0964a849488f1c4"
  !insertmacro LGSM_REMOVE_RETIRED_RESOURCE "${prefix}" "modules\runescapedragonwilds\control\save_files.hpp" "c3b53055694e79532410b852492aa485c4b2d351347adee9611fae24b7dd54eb" "32d78fc6bb6c21631a24dccef163932fe3681d6bfcb943764c3aee81576251e0"
  !insertmacro LGSM_REMOVE_RETIRED_RESOURCE "${prefix}" "modules\runescapedragonwilds\control\save_native.cpp" "c5b2956219444f983761641d71cb079c3198793d0a807b297d5e0606878d4079" "942bc37146944ef417ba7c0005dd9d92d3c50c79c8d554ccf6fe23cbffb0f453"
  !insertmacro LGSM_REMOVE_RETIRED_RESOURCE "${prefix}" "modules\runescapedragonwilds\control\save_native.hpp" "ee36ed6df6db8acf92507fc48964909b29ab031ad960a107dae9a546ed7c298b" "4717b5345476d65a0e1da53539f1ad9ed1d73946856fdf8a3b5ccc40aa5ad74b"
  !insertmacro LGSM_REMOVE_RETIRED_RESOURCE "${prefix}" "modules\runescapedragonwilds\control\save_profile.cpp" "cf2c118b3bbcb70ce0425f96739e8cbdb3a890a44262cf50be916985dc4138f6" "9240115333b5ec537f2650c9142536be67eda98508ef4c52c065f7a947e97394"
  !insertmacro LGSM_REMOVE_RETIRED_RESOURCE "${prefix}" "modules\runescapedragonwilds\control\save_profile.hpp" "f7f3b78ebaf98a304dbd68c725398acd071beaf9e10ff8b32292de3b7ddb6187" "467ac6b82ea04151edd3bdf28cf4d25e4aa217e86d7148f952bcb35bba7d1eb9"
  !insertmacro LGSM_REMOVE_RETIRED_RESOURCE "${prefix}" "modules\runescapedragonwilds\control\save_profile_data.hpp" "cb0778f77a1ac926bd8b1702785879dd5e18376e5572eed90c19b3e0ef80aac9" "fc7fccad6971f9071dbd1fe21a6f8e9e783a8ead89efb08c9297e4303fc04ef9"
  !insertmacro LGSM_REMOVE_RETIRED_RESOURCE "${prefix}" "modules\runescapedragonwilds\control\save_sinks.cpp" "032e877f3f2b49831159a1484d59458c5349ac7f2d80dd491f3a7a135fa0b66f" "9ee2b6b092cee162aae105acc4a5f773c64ba4b5d7f8da538393a4e311033325"
  !insertmacro LGSM_REMOVE_RETIRED_RESOURCE "${prefix}" "modules\runescapedragonwilds\control\tests\bridge_tests.cpp" "bd44ad6bf42db49693ff15d1f7f489a65f26d3d079151268444d145a7b7b6611" "b49368f2b958a3a4adbb02be8fa1bf10b5a1456a903d91cb8d474ea5cb903784"
  !insertmacro LGSM_REMOVE_RETIRED_RESOURCE "${prefix}" "modules\runescapedragonwilds\control\tests\completion_tests.cpp" "db7de00ca86a3d28a65e0f3598fe8a904327cad00f93cbdf14fe6291dfcc00e4" "eb68edc3567b1253fc23e9fa2f3f46d4051b631ae9a6dfc6bcb3154028a510f6"
  !insertmacro LGSM_REMOVE_RETIRED_RESOURCE "${prefix}" "modules\runescapedragonwilds\control\tests\files_tests.cpp" "4ef0b329691e5ad802a89f8f3db14ae51ced0251a6287c93eec4748df1a60c44" "5f5a24f8ea142a25628e513e935d67daf4a123cc535def3457d56a96fe4d9f48"
  !insertmacro LGSM_REMOVE_RETIRED_RESOURCE "${prefix}" "modules\runescapedragonwilds\control\tests\hooks_tests.cpp" "a2ed1c9965b9b514b68e8bbc9abeed9b3d1bfea9ef59cdb3abf8fb6e438158a1" "c6cbca3cbe26426b2216813b17ba5fed1b80909326f4409ab0d14dadebf95be6"
  !insertmacro LGSM_REMOVE_RETIRED_RESOURCE "${prefix}" "modules\runescapedragonwilds\control\tests\sinks_tests.cpp" "628b8dd006c0650b95483a09a690ce82ba02ee1e95910720eb4ca2135b34a97b" "da126ccf3004363240b28c860de9dcb503b7c46396472dfc251f87288d26936d"
  !insertmacro LGSM_REMOVE_RETIRED_RESOURCE "${prefix}" "modules\vrising\control\VRisingSaveCompletion.cs" "0bfbada5566761a02b6e55911695f3a962cc13772cceaa6a171a8507702b8a1b" "ddeeaa678545b049232d539df5ec855f88033b72cca374c7a1d5a1b1ad3d8f73"
  !insertmacro LGSM_REMOVE_RETIRED_RESOURCE "${prefix}" "modules\vrising\control\native_bridge.cpp" "3563300ebd673c657826d245db521e2e8100be8d4d68df74f086ba9ef9de9dc0" "80163e358945bd1cc9679697325dc0c2892471fc823d3c9127a8e29835d72051"
  !insertmacro LGSM_REMOVE_RETIRED_RESOURCE "${prefix}" "modules\vrising\control\native_completion.h" "9ae47f20aab53a805dedb32dd6e43041ac835091b0b526ef973d9831cfbff92f" "749b8decb4cb0b23149eead323c71a4a3e2d5bc09260ea92b9bb45fd84669856"
  !insertmacro LGSM_REMOVE_RETIRED_RESOURCE "${prefix}" "modules\vrising\control\native_unwind.cpp" "5965e5f6e85b6d74f7c886a637bc7b03bbb60b7a663081a574b9d0154dff686f" "2a23ad8472ab12a7fd14adc4c3189a578fcec119e0c9b0c801750eed5da61d02"
  !insertmacro LGSM_REMOVE_RETIRED_RESOURCE "${prefix}" "modules\vrising\control\native_unwind.h" "fba785d6c7c7ba4f97adfcafd286dfdd098ac37cd091dcafdbce050b6331e3ba" "99d475c694cfea9304909e61503904ce16f7649d89b7bc699daafe73c09843fc"
  !insertmacro LGSM_REMOVE_RETIRED_RESOURCE "${prefix}" "modules\vrising\control\tests\native_completion_tests.cpp" "b59309e93f1cd6a107932421111254fb8bdedbe156f7dec512ae490401d67af1" "ff54d2d5b6e00c26a43020e641c7e9d0153d32d0ce720ab2a6e738b68da09857"
  !insertmacro LGSM_REMOVE_RETIRED_RESOURCE "${prefix}" "modules\vrising\control\tests\native_image_probe.py" "18b9124d86ae5e8cd32ecb38ed5ee4583915c3b74f102f167333c9a685e613f8" "c26b55119cd0a35fa295ad45d9c7a62fde4e772a251f7497828a379721d8daa7"
  !insertmacro LGSM_REMOVE_RETIRED_RESOURCE "${prefix}" "modules\vrising\control\tests\native_probe.asm" "f3c66fb79988f7dde7e44c0b9b1b81fa44d42fb013553577583df42c438c78bc" "f12b370413b2f74b0ddd3baf9ae6976095061ce9ce7cf88a0b136a5ca16840af"
  !insertmacro LGSM_REMOVE_RETIRED_RESOURCE "${prefix}" "modules\vrising\control\tests\native_unwind_tests.cpp" "4a46b5fdaf53ce880cfe4a6a58044f390abaff4fb806bffa932298505c13a510" "201fb1234b67545fb65a10ee03eaceca7d4a72b41861efa1a864ed78fedf0cf5"
  !insertmacro LGSM_REMOVE_RETIRED_RESOURCE "${prefix}" "modules\enshrouded\config-fixtures\2026-09-30-native_ban_contract_build_23178631.json" "a7e2731217a7522f6d28bf0355cdfa098253d1d90e2c6d4c1594e407de239719" "ccff88d90223848426dc7ac8ecb0c9969d497b1ee886c07e9b28303a52810b60"
  !insertmacro LGSM_REMOVE_RETIRED_RESOURCE "${prefix}" "modules\terraria\config-fixtures\2026-09-28-official_dedicated_zip_1458.json" "55860a78dc8e8640923556b8469ab9d375b8c82fc36e815a9dd9908160d75ec5" "fd5de98799c4655ee34dc4c232f58a804fcdab066be290b6e7cda1ba56c8b873"
  ClearErrors
!macroend
!endif
