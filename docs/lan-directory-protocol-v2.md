# LanGame LAN Directory Protocol v2 / LanGame 局域网目录协议 v2

[English](#english) | [简体中文](#简体中文)

## English

This document describes the v2 multicast sender implemented by `apps/desktop/src-tauri/src/lan_directory.rs`. It is a discovery protocol for bounded, read-only status. It is not a remote-management protocol.

### Transport

| Property | v2 behavior |
| --- | --- |
| Network | IPv4 UDP multicast |
| Destination | `239.255.76.71:47671` |
| Sender bind | One socket per eligible active IPv4 address, with an operating-system-assigned source port and an explicit multicast outgoing interface |
| Multicast TTL | `1` |
| Multicast loopback | Enabled |
| Socket mode | Non-blocking |
| Normal cadence | One publish cycle approximately every 5 seconds |
| Maximum datagram | 1,200 bytes, including the compact UTF-8 JSON payload |
| Instance bound | At most the first 128 running instances considered per cycle |
| Sender retry | 1 second, doubled after each sender failure, capped at 30 seconds |

The sender enumerates active IPv4 interfaces and binds a separate socket to each eligible address. Both the source address and multicast outgoing interface are explicit, so a VPN or default route cannot redirect the physical LAN's announcements. Eligible addresses are RFC 1918, IPv4 link-local, `25.0.0.0/8`, `26.0.0.0/8`, and `100.64.0.0/10`; loopback, public, inactive, and proxy benchmarking addresses are excluded. At most 64 distinct addresses are supported, with an explicit diagnostic if enumeration exceeds this bound. Interfaces are refreshed at least once per normal publish interval.

Each interface uses the same process-session node ID and independently projects joins against its actual outgoing address. This preserves the rule that a join descriptor may only describe an instance bound to that interface or to all addresses.

### Publish cycle and framing

The sender takes a snapshot of running instances, builds one node event and zero or more server events, and publishes only when at least one valid server event remains.

When a cycle is publishable, datagrams are sent in this order:

1. one node event;
2. one server event for each selected instance.

Each event is serialized as one compact UTF-8 JSON object in its own UDP datagram. There is no envelope, array, delimiter, newline, sequence number, or transaction boundary. A receiver can therefore observe loss, duplication, reordering, or a partial cycle. JSON object member order is not semantically significant, even though the current sender has deterministic field order.

Stopped instances are omitted. No explicit departure or tombstone event is sent. If no valid running server exists, the node event is also omitted.

### Schema identifiers

Receivers distinguish event types by an exact `schema` string:

| Event | Schema |
| --- | --- |
| Node | `cn.langame.lgsm-directory.node.v2` |
| Server | `cn.langame.lgsm-directory.server.v2` |

The current sender emits only these v2 schemas. Behavior for unknown fields is not defined by the sender. A receiver should select the exact schema it supports and ignore unsupported schema identifiers.

The `lgsm-directory` namespace is frozen as part of the v2 wire contract. It is a protocol identifier, not a current product name, and must not be renamed without a protocol version bump.

### Node event

| Field | JSON type | Sender behavior |
| --- | --- | --- |
| `schema` | string | Always `cn.langame.lgsm-directory.node.v2` |
| `node_id` | string | A UUID v4 rendered as 32 lowercase hexadecimal characters; stable across broadcaster recovery within one application process and regenerated after an application restart |
| `node_name` | string | Sanitized `COMPUTERNAME`, limited to 128 UTF-8 bytes; falls back to `LanGame Node` |
| `emitted_at` | unsigned integer | Unix time in milliseconds; shared by all events built in the same cycle |

Golden payload:

~~~json
{"schema":"cn.langame.lgsm-directory.node.v2","node_id":"0123456789abcdef0123456789abcdef","node_name":"LanGame Test Node","emitted_at":1700000000000}
~~~

### Server event

| Field | JSON type | Sender behavior |
| --- | --- | --- |
| `schema` | string | Always `cn.langame.lgsm-directory.server.v2` |
| `node_id` | string | The node ID from the same process session |
| `instance_id` | string | Trimmed protocol ID, limited to 128 ASCII bytes |
| `name` | string | Sanitized instance display name, limited to 128 UTF-8 bytes; falls back to `instance_id` |
| `module_id` | string | Trimmed protocol ID, limited to 128 ASCII bytes |
| `module_name` | string | Sanitized module display name, limited to 128 UTF-8 bytes; falls back to `module_id` |
| `running` | boolean | Always `true`; non-running instances are not published |
| `join` | object or null | A verified join projection, or `null` when it cannot be produced |
| `emitted_at` | unsigned integer | Unix time in milliseconds; equal to the node event timestamp for the cycle |

The only v2 join object currently emitted is:

| Field | JSON type | Meaning |
| --- | --- | --- |
| `kind` | string | Always `steam_connect` |
| `client_app_id` | unsigned integer | The non-zero client application ID declared by the module |
| `join_port` | unsigned integer | The instance's current named join port |
| `query_port` | unsigned integer | The instance's current named UDP query port |

Golden payload:

~~~json
{"schema":"cn.langame.lgsm-directory.server.v2","node_id":"0123456789abcdef0123456789abcdef","instance_id":"rust-one","name":"Rust One","module_id":"rust","module_name":"Rust Dedicated Server","running":true,"join":{"kind":"steam_connect","client_app_id":252490,"join_port":31015,"query_port":31017},"emitted_at":1700000000000}
~~~

When no join projection is available, `join` remains present with a JSON `null` value.

### Selection and normalization

Running instances are sorted by their stored instance ID. The sender considers at most the first 128, then omits any event whose instance or module ID is invalid.

A protocol ID is valid only when, after trimming surrounding whitespace, it:

- is non-empty;
- is no longer than the field's byte limit; and
- contains only ASCII letters, ASCII digits, hyphen (`-`), underscore (`_`), period (`.`), or colon (`:`).

Display text is normalized before publication:

- Unicode control characters are removed;
- bidirectional formatting controls U+061C, U+200E, U+200F, U+202A–U+202E, and U+2066–U+2069 are removed;
- whitespace runs are collapsed to one ASCII space, with no leading or trailing space;
- text is truncated at a valid UTF-8 boundary to the field's byte limit; and
- an empty result uses the documented fallback, then `Unknown` if the fallback is also empty.

These rules limit framing and display abuse; they do not make operator-provided names confidential or trustworthy.

### Join projection

The server event itself does not carry a host field. A receiver uses the IPv4 source address of the UDP datagram as the host and combines it with a published port only after validating the event.

The sender emits a `steam_connect` object only when all of the following are true:

- the module declares a supported `runtime.join` profile;
- the instance's bind address parses as IPv4;
- the bind address is `0.0.0.0` or exactly equals the UDP sender's concrete source address;
- persisted instance ports contain exactly one named join binding and exactly one named query binding;
- both actual ports are non-zero; and
- the actual query binding protocol is exactly `udp`.

Otherwise `join` is `null`. A missing profile, missing or duplicate port, invalid bind address, source-address mismatch, zero port, non-UDP query binding, storage failure, or module-discovery failure cannot suppress the base server event. A projection-wide failure instead causes that cycle's server events to carry `join: null`.

Module join profiles are cached for no more than 30 seconds when the module root and module ID/version signature are unchanged. Actual instance port projections are read for each cycle.

### Failure and timing behavior

Socket-creation, encoding, and send failures are reported per interface and retry independently with exponential backoff from 1 to 30 seconds. Healthy interfaces keep their five-second cadence and their existing sockets. Interface enumeration failures are reported while previously bound senders continue; removed interfaces are closed after a successful enumeration. State-snapshot failures apply the same bounded backoff to the shared snapshot. Successful attempts reset the corresponding backoff. Shutdown checks occur at intervals no longer than 100 milliseconds while waiting.

`emitted_at` is derived from the local system clock and is not signed or guaranteed monotonic. It can be zero if the system clock cannot be converted to Unix time. The protocol does not define a receiver expiration interval. Receivers should use a bounded local freshness policy that accounts for the normal five-second cadence and should not treat `emitted_at` as proof of origin or freshness.

### Security boundary

LanGame LAN Directory v2 provides no authentication, authorization, confidentiality, integrity, replay protection, delivery guarantee, or discovery-level rate negotiation.

- Any host able to send traffic into the multicast domain may forge or replay a syntactically valid event.
- TTL 1 limits normal IP routing but does not establish trust, same-subnet membership, or privacy.
- Node names, instance names, module names, ports, timestamps, and the network-layer source address are visible to listeners. Do not put secrets or personal data in names.
- The payload intentionally contains no password, token, management credential, filesystem path, host field, player data, or management action.
- A receiver must treat every datagram as untrusted input, enforce the 1,200-byte limit before parsing, validate types and ranges, and tolerate malformed, duplicated, reordered, and missing events.
- Discovery must never authorize an administrative action or automatically execute a command, open a file, download content, or connect with stored credentials.
- A join descriptor is only a connection hint. The game server remains responsible for its own authentication, authorization, and access policy.

This document specifies sender behavior only. A receiver's cache, UI, join confirmation, and abuse controls are separate security decisions.

## 简体中文

本文说明 `apps/desktop/src-tauri/src/lan_directory.rs` 当前实现的 v2 组播发送端。该协议只用于发现有明确边界的只读状态，不是远程管理协议。

### 传输

| 属性 | v2 行为 |
| --- | --- |
| 网络 | IPv4 UDP 组播 |
| 目标地址 | `239.255.76.71:47671` |
| 发送端绑定 | 每个符合条件的活动 IPv4 地址使用独立 Socket，源端口由操作系统分配，并显式指定组播出口 |
| 组播 TTL | `1` |
| 组播回环 | 启用 |
| Socket 模式 | 非阻塞 |
| 正常周期 | 约每 5 秒发布一轮 |
| 数据报上限 | 1,200 字节，包含紧凑 UTF-8 JSON 载荷 |
| 实例上限 | 每轮最多考虑排序后的前 128 个运行中实例 |
| 发送端重试 | 从 1 秒开始，每次发送端故障后翻倍，最高 30 秒 |

发送端枚举活动 IPv4 接口，为每个符合条件的地址分别绑定 Socket，并显式指定源地址和组播出口，避免 VPN 或默认路由将物理局域网广播导向其他网络。支持 RFC 1918、IPv4 链路本地地址、`25.0.0.0/8`、`26.0.0.0/8` 和 `100.64.0.0/10`；排除回环、公网、未活动和代理基准测试地址。最多支持 64 个不同地址，超过此数量时明确报告。接口列表至少在每个正常广播间隔刷新一次。

各接口使用相同的进程会话节点 ID，并分别按实际出口地址生成加入描述；只有绑定该接口或全部地址的实例才能获得有效加入描述。

### 发布轮次与封装

发送端获取运行中实例快照，构建一个节点事件和零个或多个服务器事件。仅当至少保留一个有效服务器事件时才会发布。

可发布轮次按以下顺序发送数据报：

1. 一个节点事件；
2. 每个选中实例各一个服务器事件。

每个事件都序列化为独立 UDP 数据报中的一个紧凑 UTF-8 JSON 对象。协议不包含外层信封、数组、分隔符、换行符、序列号或事务边界。接收端可能观察到丢失、重复、乱序或不完整轮次。当前发送端的字段顺序是确定的，但 JSON 对象成员顺序不具备协议语义。

停止的实例不会发布，也没有显式离开或墓碑事件。没有有效运行中服务器时，节点事件同样不会发布。

### Schema 标识

接收端使用准确的 `schema` 字符串区分事件类型：

| 事件 | Schema |
| --- | --- |
| 节点 | `cn.langame.lgsm-directory.node.v2` |
| 服务器 | `cn.langame.lgsm-directory.server.v2` |

当前发送端只发布上述 v2 Schema。发送端没有规定未知字段的处理方式。接收端应准确选择自身支持的 Schema，并忽略不支持的 Schema 标识。

`lgsm-directory` 命名空间已经固化为 v2 线上契约的一部分。它是协议标识，不代表当前产品名称；如需重命名，必须同步提升协议版本。

### 节点事件

| 字段 | JSON 类型 | 发送端行为 |
| --- | --- | --- |
| `schema` | 字符串 | 固定为 `cn.langame.lgsm-directory.node.v2` |
| `node_id` | 字符串 | UUID v4 的 32 位小写十六进制形式；同一应用进程内发送器恢复后保持不变，应用重启后重新生成 |
| `node_name` | 字符串 | 清理后的 `COMPUTERNAME`，最多 128 个 UTF-8 字节；回退值为 `LanGame Node` |
| `emitted_at` | 无符号整数 | Unix 毫秒时间戳；同一轮构建的所有事件使用相同值 |

准确示例：

~~~json
{"schema":"cn.langame.lgsm-directory.node.v2","node_id":"0123456789abcdef0123456789abcdef","node_name":"LanGame Test Node","emitted_at":1700000000000}
~~~

### 服务器事件

| 字段 | JSON 类型 | 发送端行为 |
| --- | --- | --- |
| `schema` | 字符串 | 固定为 `cn.langame.lgsm-directory.server.v2` |
| `node_id` | 字符串 | 同一进程会话中的节点 ID |
| `instance_id` | 字符串 | 去除首尾空白后的协议 ID，最多 128 个 ASCII 字节 |
| `name` | 字符串 | 清理后的实例显示名称，最多 128 个 UTF-8 字节；回退到 `instance_id` |
| `module_id` | 字符串 | 去除首尾空白后的协议 ID，最多 128 个 ASCII 字节 |
| `module_name` | 字符串 | 清理后的模块显示名称，最多 128 个 UTF-8 字节；回退到 `module_id` |
| `running` | 布尔值 | 固定为 `true`；不发布非运行中实例 |
| `join` | 对象或 null | 已验证的加入映射；无法生成时为 `null` |
| `emitted_at` | 无符号整数 | Unix 毫秒时间戳，与本轮节点事件相同 |

v2 当前只发布一种加入对象：

| 字段 | JSON 类型 | 含义 |
| --- | --- | --- |
| `kind` | 字符串 | 固定为 `steam_connect` |
| `client_app_id` | 无符号整数 | 模块声明的非零客户端 App ID |
| `join_port` | 无符号整数 | 实例当前使用的命名加入端口 |
| `query_port` | 无符号整数 | 实例当前使用的命名 UDP 查询端口 |

准确示例：

~~~json
{"schema":"cn.langame.lgsm-directory.server.v2","node_id":"0123456789abcdef0123456789abcdef","instance_id":"rust-one","name":"Rust One","module_id":"rust","module_name":"Rust Dedicated Server","running":true,"join":{"kind":"steam_connect","client_app_id":252490,"join_port":31015,"query_port":31017},"emitted_at":1700000000000}
~~~

没有可用加入映射时，`join` 字段仍然存在，其 JSON 值为 `null`。

### 选择与规范化

运行中实例按存储的实例 ID 排序。发送端最多考虑前 128 个实例，随后省略实例 ID 或模块 ID 无效的事件。

协议 ID 去除首尾空白后，必须同时满足以下条件：

- 非空；
- 不超过对应字段的字节上限；
- 只包含 ASCII 字母、ASCII 数字、连字符（`-`）、下划线（`_`）、句点（`.`）或冒号（`:`）。

显示文本在发布前按以下规则处理：

- 删除 Unicode 控制字符；
- 删除双向格式控制字符 U+061C、U+200E、U+200F、U+202A–U+202E 和 U+2066–U+2069；
- 将连续空白折叠为一个 ASCII 空格，并删除首尾空格；
- 在有效 UTF-8 边界处截断到字段字节上限；
- 结果为空时使用规定的回退值；回退值仍为空时使用 `Unknown`。

这些规则用于限制封装和显示攻击，不能让运维人员提供的名称变得机密或可信。

### 加入映射

服务器事件不携带主机字段。接收端应将 UDP 数据报的 IPv4 源地址作为主机，并仅在验证事件后与发布端口组合使用。

发送端仅在以下条件全部满足时发布 `steam_connect` 对象：

- 模块声明受支持的 `runtime.join` 配置；
- 实例绑定地址可解析为 IPv4；
- 绑定地址为 `0.0.0.0`，或与 UDP 发送端的明确源地址完全一致；
- 持久化实例端口中，命名加入绑定和命名查询绑定分别只有一个；
- 两个实际端口均不为零；
- 实际查询绑定协议准确为 `udp`。

否则 `join` 为 `null`。缺少配置、端口缺失或重复、绑定地址无效、源地址不一致、端口为零、查询绑定不是 UDP、存储失败或模块发现失败都不会阻止基础服务器事件发布。整个映射加载失败时，本轮服务器事件会改为携带 `join: null`。

模块根目录和模块 ID/版本签名不变时，模块加入配置最多缓存 30 秒。每轮都会重新读取实例的实际端口映射。

### 故障与时间行为

Socket 创建、编码或发送失败按接口报告，并分别按 1 秒到 30 秒的指数退避重试。健康接口保留原 Socket 和 5 秒广播间隔。接口枚举失败时明确报告，之前已绑定的发送器继续工作；枚举成功后关闭已移除接口的 Socket。状态快照失败对共享快照采用相同的有界退避，成功后重置对应退避。等待期间检查关闭状态的间隔不超过 100 毫秒。

`emitted_at` 来自本机系统时钟，未经签名，也不保证单调递增。系统时钟无法转换为 Unix 时间时，该值可能为零。协议没有规定接收端过期时间。接收端应根据正常的 5 秒发布周期制定有上限的本地新鲜度策略，不得将 `emitted_at` 视为来源或新鲜度证明。

### 安全边界

LanGame 局域网目录协议 v2 不提供身份认证、授权、机密性、完整性、重放保护、送达保证或发现层速率协商。

- 任何能够向组播域发送流量的主机都可能伪造或重放语法有效的事件。
- TTL 1 只限制正常 IP 路由，不能建立信任、同子网成员关系或隐私边界。
- 节点名、实例名、模块名、端口、时间戳和网络层源地址对监听者可见。名称中不得包含密钥或个人数据。
- 载荷特意不包含密码、令牌、管理凭据、文件系统路径、主机字段、玩家数据或管理操作。
- 接收端必须将每个数据报视为不可信输入，在解析前执行 1,200 字节上限，校验类型与范围，并容忍畸形、重复、乱序和缺失事件。
- 发现结果不得授权管理操作，也不得自动执行命令、打开文件、下载内容或使用已存储凭据连接。
- 加入描述只是一条连接提示。游戏服务器仍负责自身的身份认证、授权和访问策略。

本文只规定发送端行为。接收端缓存、界面、加入确认和滥用防护属于独立的安全决策。
