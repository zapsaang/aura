**TL;DR: AURA 产品设计白皮书核心摘要**
* **产品定位**：AURA (Asynchronous Ultra-low-overhead Resource Agent) 是一款专为极客和底层开发者设计的纳秒级系统遥测探针。
* **核心革命**：彻底消灭终端 UI 刷新时的进程 Fork 和文本解析开销。采用 C/S 架构，基于共享内存 (mmap) 和无锁序列化 (bytemuck) 实现零系统调用的数据直出。
* **运行平台**：跨平台原生支持。在 Linux 系统下无缝对接底层内核接口，在 macOS 下原生调用 Mach 端口，统一由操作系统的顶级进程管理器兜底保活。

### 1. 核心设计哲学 (The Philosophy)

* **零计算下放**：前端 UI（Tmux/Zellij）只配做无脑的字符串展示。任何耗费 CPU 周期的浮点数运算、类型转换、状态判断，必须全部收敛在后台守护进程中完成。
* **拒绝文本协议**：抛弃标准输出管道和 UNIX Socket。使用内存映射文件作为唯一的 IPC 媒介，彻底消除数据传递过程中的内核态/用户态上下文切换。
* **绝对的不信任**：AURA 对自身的稳定性保持极度怀疑。不手写进程管理，将守护进程的命脉完全移交给操作系统的 Init 系统，并强制引入 Watchdog 硬件级心跳机制防假死。

---

### 2. 系统架构蓝图 (Architecture Blueprint)

AURA 物理上分裂为两个绝对解耦的二进制可执行文件：

* **核心引擎：`aura-daemon` (生产者)**
  * **运行模式**：常驻后台的独立进程。
  * **心跳周期**：默认 500ms 唤醒一次（可通过配置文件动态调整）。
   * **数据管线**：调用原生系统 API 采集数据 -> 将数据结构填充入内存 -> 使用 `bytemuck` 执行零拷贝序列化 -> 更新无锁环形缓冲区（SeqLock）的版本号 -> 进入纳秒级休眠。
  * **保活机制**：每次成功写入内存后，向内核发送 `WATCHDOG=1` 信号。若主循环阻塞超 3 秒，由系统强制猎杀并拉起。

* **极简探针：`aura-cli` (消费者)**
  * **运行模式**：由终端多路复用器按秒级频率拉起的瞬态进程。
  * **执行逻辑**：解析命令行参数 (如 `--module cpu`) -> 读取 mmap 内存块校验版本号 -> 指针强转获取数据 -> 对比时间戳防止读到“僵尸数据” -> 格式化输出 -> 进程销毁。
  * **性能指标**：整个生命周期耗时必须严格控制在微秒级，对宿主系统 CPU 的扰动接近 0%。

---

AURA 采集的数据必须具备足够的工程深度，以支撑未来对底层存储引擎进行极限性能压测。结合全面接管的系统状态监控需求，数据载荷被重新划分为以下几大核心维度：

* **计算维 (Compute)**：
  * 除了基础的全局/单核利用率，必须包含核心的**上下文切换频率 (Context Switches/s)**。这是排查高并发锁竞争的直接证据。
  * 精准拆解全局以及每个独立核心（如 cpu0, cpu1）的用户态 (loadUser)、内核态 (loadSystem)、空闲 (loadIdle) 和总时间片 (loadTotal) 滴答数。
* **进程维 (Process - 新增)**：
  * **全局拓扑**：瞬间抓取当前系统的总进程数 (all)，以及处于运行中 (running)、阻塞 (blocked) 和睡眠 (sleeping) 状态的精确快照。
  * **性能杀手**：在后台静默维护 Top 5 消耗 CPU (topsCostCpu) 和 Top 5 消耗内存 (topsCostMemory) 的进程列表，透出其 PID、绝对内存占用、CPU 占用百分比及执行命令。AURA 守护进程必须通过直接解析 `/proc/[pid]/stat` 来完成此事，绝不允许调用 `ps`。
* **内存维 (Memory)**：
  * 物理内存与 Swap 的绝对值（总量 total、空闲 free、已用 used、Swap总量 swapTotal、Swap已用 swapUsed、Swap空闲 swapFree）。
  * 必须单独透出 Buffers/Cache (buffcache) 的占用量，这对于评估文件系统缓存命中率至关重要。
  * 包含**缺页中断率 (Page Faults/s)**，直击程序的内存分配效率。
* **存储与 I/O 维 (Storage & Block I/O)**：
  * **吞吐与延迟**：不仅要透出磁盘队列深度和 IOPS，还需精确到具体底层块设备（如 vda）的每秒读取字节数 (rxPerSec) 和每秒写入字节数 (wxPerSec)。
  * **空间拓扑**：遍历所有挂载的真实文件系统，映射出每个分区（如 /dev/vda1）的文件系统类型 (type)、总空间 (size)、可用空间 (available)、使用百分比 (percent) 以及确切的挂载点 (mount)。
* **网络维 (Network - 新增)**：
  * **流量审计**：计算真实网络接口的周期内总接收字节数 (rxBytes) 和发送字节数 (txBytes) 的差值。
  * **速率监控**：提供精准的瞬时下行速率 (rxBytesPerSec) 和上行速率 (txBytesPerSec)。
* **环境与元数据维 (Environment & Meta)**：
  * **系统指纹**：系统负载 (Load Average)、在启动时就缓存好的发行版精确画像（包含系统类型 type、ID、版本 version/versionId、完整名称 prettyName 以及代号 versionCodename）。彻底干掉运行时的发行版猜测逻辑。
  * **时间信标**：系统绝对时间戳 (timestamp)、高精度开机运行时长 (uptime)、时区偏移（如 GMT+0800）及本地时区名称（如 CST）。
* **异构计算维 (GPU/Accelerators)**：
  * 若探测到 GPU 硬件，需通过原生 NVML C API 或类似底层调用（严禁 `exec` 调用 `nvidia-smi`）抓取显卡名称、显存总量及使用量、核心利用率、实时功耗及温度。

---

### 4. 交付与部署标准 (Deployment & Ecosystem)

为了匹配现代声明式环境的管理哲学，AURA 不提供繁琐的手动安装脚本。

* **配置下发**：无论是 Linux 还是 macOS，统一通过 Home Manager 进行源码编译与环境注入。
* **Linux 落地**：生成 `systemd.user.services.aura-daemon` 单元文件，绑定 `default.target` 实现登录即拉起，接管所有 stdout 日志流。
* **macOS 落地**：生成 `LaunchAgents` 下的 `.plist` 属性列表文件，利用 macOS 底层机制实现静默驻留。

---

### 5. 多模态交互接口 (CLI Interface)

`aura-cli` 必须提供极度克制且精确的路由系统，绝不允许出现 `grep` 过滤：

* **模块化输出**：`aura-cli --module <cpu|mem|swap|disk|net|os>`，要什么给什么。
* **色彩引擎开关**：`aura-cli --module cpu --color <tmux|zellij|ansi|none>`，在底层完成阈值判断（如 CPU > 80% 标红）并直接输出对应 UI 框架的色彩转义码。
* **降维兜底机制**：一旦检测到共享内存数据过期超过 2 秒，所有模块强制输出隐晦的宕机提示（如 ` ---`），绝不能抛出 Panic 堆栈破坏前端排版。
* **上帝视角接口**：`aura-cli --format json`，直接吐出结构化全量数据。这为你后续将数据回传给自定义 Web Dashboard 或 SSH 落地页留下了原生支持。

