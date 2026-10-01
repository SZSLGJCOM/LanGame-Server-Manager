# Desktop UI / 桌面界面

## English

### Typography

`apps/desktop/src/styles/typography.css` owns font families, text roles, weights and
line heights. The root is 16 CSS pixels; ordinary text uses these rem-based roles:

| Role | Token | Logical size |
| --- | --- | --- |
| Page title | `--text-page-title` | 22px |
| Section title | `--text-section-title` | 16px |
| Body, navigation, buttons and form controls | `--text-body` | 13px |
| Supporting information | `--text-secondary` | 12px |
| Time, counts and metadata | `--text-meta` | 12px |
| Console output, commands and code | `--text-code` | 13px |

Use `--font-sans` for interface text and `--font-mono` for code. Interface Latin
text and numbers use the bundled Inter 4.1 variable fonts (upright and italic);
no CDN or OS font installation is needed. Chinese glyphs fall back to the
configured system Chinese fonts. Body and input text use weight 400, ordinary
buttons/navigation/form labels use 500, and headings use 600. Use shared weight
and line-height tokens instead of introducing local scales. Instrument readings and
decorative artwork have separate display roles; their sizing must not alter
ordinary text. Window width changes layout, not the text scale.

Standard action buttons use a 30px minimum height with 10px horizontal padding;
compact actions use 28px and 8px. Both use the shared control tokens and 1.25
line height. Multi-line labels may grow rather than clip. Navigation strips and
native window controls retain their existing geometry. Text and button density
do not scale the surrounding panels or change their column proportions, gaps or
outer padding. Display readings use 28px, with 54px/40px central dial roles;
ordinary supporting text stays at least 12px and console text stays 13px.

Configuration and Maintenance share the controls in `styles/settings-controls.css`.
Single-line fields and toggle rows use a 38px minimum height and 8px radius;
actions beside a field use the same 38px height. Standalone actions remain 30px,
and compact icon actions use 28px. Labels, help and multiline content may grow
without clipping. Specialized editors use these roles rather than local sizes.
Help uses the shared field tooltip for hover and keyboard focus, including paths
and disabled-action explanations; Escape dismisses it. Do not mix native `title`
bubbles with this help or repeat a visible label as a tooltip. Preserve meaningful units, conditions and native input syntax across sentences; hide provenance-only text and leave unauthored help empty.

Hover help waits for a 240ms stay and cancels pending appearance when the pointer
leaves. Only one help bubble may be active. Mouse clicks must not pin help through
retained control focus; keyboard focus opens help immediately. Allow a 100ms gap
crossing into readable help, followed by a 140ms exit when both regions are left.
Escape, window blur, pointer cancellation and leaving the viewport dismiss help;
scrolling outside the bubble dismisses pointer help. Preserve scrolling inside
long help. `hover-help-browser.test.cjs` verifies these interactions with native
browser mouse/keyboard input and explicit window/pointer lifecycle events.

Game details keep installation actions, creation choices and compact program
metrics in the sidebar. Paths, versions and reuse explanations belong in those
controls' tooltips; loading, blocked creation and retry remain discoverable.
The sidebar aligns with the complete media card, while the player stays 16:9.
Creation actions stay at the bottom; metadata scrolls when height is constrained.

The original font license is shipped at `fonts/inter/OFL.txt`. Provenance and
file digests are recorded in [the asset ledger](../THIRD_PARTY_ASSETS/NOTICE).

### Window and layout

The desktop starts windowed, with a preferred client size of 1560×900 logical
pixels. Before showing each new or recovered window, fit it to the current
monitor's work area, accounting for DPI, taskbars, frame dimensions and monitors
with negative coordinates. The usual minimum is 960×600; reduce it when the work
area is smaller. Resizing and maximizing remain available. A monitor or DPI
change refits overflowing bounds; ordinary resizing on the same monitor retains
the user's size. Native window state owns maximize/restore behavior. Window size
is not persisted across application restarts or WebView reconstruction.

Configuration categories collapse at widths up to 1100 CSS pixels. Selecting a
category closes the compact navigation and returns focus to its toggle; selecting
a search result focuses its editor. Form columns respond to the available content
container. Server lists, consoles and configuration content retain local scrolling;
workflow tabs can scroll horizontally to keep their labels readable. Short system
dashboards scroll rather than compressing their instrument and content to zero.
Below 500 CSS pixels in height, the server page also scrolls inside the shell,
preserving enough workspace height for complete cards and controls.

### Operation feedback

Show operation results, warnings and request failures in the shared bottom
activity bar through `ActivityNotice`, retaining retry actions where applicable.
Feedback must not insert banners into instance lists or resize their content.
Keep loading and empty states in their content area; object-specific recovery
constraints remain with the affected object's details. Dismissing feedback does
not turn a failed read into a successful empty result.

The activity bar remains one 54px-high row. Concurrent messages share one
rotating slot with previous/next, pause and full-message controls; the task
progress and cancellation control retain their own space. Rotation pauses on
hover, keyboard focus, expanded details, hidden windows and reduced-motion
preference. Details open above the bar without moving the workspace and expose
selectable, wrapped diagnostics and recovery actions, including task failures.
Messages remain owned by their originating view; the panel is not a history log.

### Application updates

Finding a new application version opens an update prompt, independent of LAN,
without a header control. It offers the version's GitHub download page or an
in-app update directly. The prompt explains that an in-app update downloads,
requests server saves, stops servers, installs and restarts; selecting that
action starts the operation without a second confirmation screen. Later,
close and Escape do not approve installation. Dismissing the prompt suppresses
automatic repeats for the same version in the current session. Progress and
failure recovery remain in the shared activity bar after closing the prompt.
Disabled builds, initial checks and up-to-date results do not open a prompt.

### Verification

`typography-contract.test.cjs` guards the shared role definitions and their use.
It also checks the bundled WOFF2 faces and license; the browser matrix waits
for font loading before checking layout and inspects the rendered UI font.
`desktop-layout-browser.test.cjs` exercises the real shell, server workspace and
configuration controls with synthetic storage data at 960×600, 1280×720,
1560×900 and 1920×1080 CSS viewports, plus DPR 1.25, 1.5 and 2. Short work areas
also cover 960×520 at DPR 2 and 853×453 at DPR 1.5. The browser checks
actual computed typography, clipping, scrolling, language/theme changes and
keyboard interactions. Its DPR emulation does not replace native multi-monitor
Windows DPI testing. `desktop_window::tests` verifies work-area geometry and
the distinction between normal resizing and display changes.
`desktop-main-pages-browser.test.cjs` covers the system dashboard, game catalog
and game detail controls at 960×600 and 1560×900, using synthetic game data,
including an English-language run at 1560×900.

`app-update-browser.test.cjs` checks the real shell with synthetic update results
at 960×600 and 1560×900, including automatic prompts, dismissal, the external
download action, direct online updates, keyboard focus, themes and retry. This does not establish
real signed-installer or GitHub delivery acceptance; see [desktop releases](desktop-release.md).

## 简体中文

发现新版本后自动弹出更新提示，直接提供“前往下载”和“在线更新”，不设顶部入口。
前者打开对应版本的 GitHub 发布页；后者直接开始应用内更新。停服影响在同一层提示中
说明：下载完成后请求保存并停止服务器，再安装和重启，不再增加第二层确认。
“稍后”、关闭和 Escape 均不授权安装，同一版本关闭后不因后台轮询重复提示。
下载、安装和失败重试反馈保留在底部动态栏；未启用更新、首次检查中或已是最新版本时
不弹出。`app-update-browser.test.cjs` 使用合成更新结果在 960×600 和 1560×900
验证自动提示、关闭去重、下载跳转、在线更新、键盘焦点、主题和重试；签名安装器与
GitHub 分发仍需按[桌面发行指南](desktop-release.md)验收。

字体规范统一维护在 `apps/desktop/src/styles/typography.css`：页面标题22px、
分区标题16px、正文和操作控件13px、辅助信息与元信息12px、代码和日志13px。
尺寸以16px根字号的 rem 表达。界面和等宽字体分别使用 `--font-sans`、
`--font-mono`，字重与行高也使用共享变量。普通文字不随窗口宽度缩放；仪表盘
数值和装饰字形使用独立展示角色。

常规操作按钮最小高度30px、水平内边距10px，紧凑按钮28px与8px，使用共享
控件变量和1.25行高；多行标签允许增高，避免裁切。顶层导航条和原生窗口
控制保持既有几何尺寸。文字与按钮的紧凑化不缩放面板，不改变区域列宽占比、
间距和外围留白。展示数字28px，仪表中心数字使用54px/40px角色；普通辅助
文字不低于12px，控制台保留13px。

配置页与维护页共用 `styles/settings-controls.css`：单行输入、选择器和开关行
最小高度38px、圆角8px，紧邻字段的操作按钮同为38px；独立操作30px，紧凑图标
操作28px。标签、说明和多行内容允许自然增高，不裁切。专用编辑器沿用这些角色，
不另设局部尺寸。帮助说明、完整路径和禁用原因统一使用字段气泡，支持悬停、
键盘聚焦及Escape关闭；不混用原生 `title` 气泡，也不重复已有可见标签。有用的单位、生效条件和原生输入语法须完整保留，不按首句截断；纯来源说明不作为帮助，没有实质说明时不生成占位文案。

鼠标停留240ms后显示帮助，提前移出须取消待显示任务；同一时刻仅保留一个气泡。
鼠标点击留下的控件焦点不能锁住气泡，键盘聚焦则立即显示。跨越控件与气泡间隙
保留100ms，离开两者后用140ms退场。Escape、窗口失焦、指针取消及移出窗口须
关闭气泡；外部滚动关闭鼠标帮助，气泡内部滚动保持可读。上述行为由
`hover-help-browser.test.cjs` 使用浏览器原生鼠标、键盘及明确的生命周期事件验证。

游戏详情侧栏仅常驻安装操作、创建选项和紧凑程序指标；路径、版本与复用说明
放入对应控件的气泡，加载、创建受阻和重试状态仍须可发现。侧栏与完整媒体卡
上下对齐，视频保持16:9；创建操作靠底，空间不足时上部资料局部滚动。

界面英文和数字使用内置的Inter 4.1可变字体，包含正体与斜体，无需在线下载或
安装系统字体。中文保持系统中文字体回退，代码和日志保持等宽字体。正文和
输入内容400字重，普通按钮、导航和表单标签500，标题600。原始OFL许可证随
前端资源分发，来源及摘要见[第三方资源台账](../THIRD_PARTY_ASSETS/NOTICE)。

默认窗口化，优先采用1560×900逻辑像素，显示前按当前显示器的可用工作区和
DPI限制尺寸并居中。通常最小尺寸为960×600，工作区更小时相应降低，避免窗口
超出屏幕。保留调整大小和最大化；切换显示器或DPI时重新校正边界，同屏普通
调整不会重置用户尺寸。重启和WebView重建不持久化此前窗口尺寸。

配置分类在1100px及以下折叠，支持键盘展开、关闭和选择；选择分类后焦点回到
展开按钮，搜索字段后焦点进入编辑器。表单列数按内容容器宽度变化。服务器列表、
终端和配置内容各自滚动；工作区标签可横向滚动，避免压缩标签文字。低高度的
系统页面允许内容滚动。高度不超过500px时，服务器页也在主内容区内滚动，
保留完整卡片和编辑器所需的空间。

操作结果、警告和请求失败通过 `ActivityNotice` 显示在统一底部动态栏，按需保留
重试入口；不要在实例列表内插入反馈框或挤压内容区域。加载和空状态留在对应
内容区，具体对象的恢复限制留在该对象详情内。关闭提示不代表失败的读取已经
成功，也不能据此将未知数据当作空列表。

动态栏保持54px高的单行：并发消息在一个展示位轮换，提供前后切换、暂停及全文
入口，任务进度与取消按钮保留独立空间。悬停、键盘聚焦、展开详情、窗口隐藏或
偏好减少动画时暂停轮播。详情浮层在动态栏上方展开，不移动工作区；完整诊断
可换行、选中，保留恢复操作及任务失败详情。消息随所属视图卸载，不作为历史日志。

验证入口见上述测试文件。浏览器矩阵验证真实组件的计算字号、裁切、滚动和
中英文键盘交互，并覆盖960×520（200%）和853×453（150%）短工作区。
系统页、游戏库和详情页另有960×600及1560×900浏览器交互检查。
浏览器DPR模拟与原生窗口几何单测不代表已经完成Windows
多显示器跨DPI拖动的实机验收。
