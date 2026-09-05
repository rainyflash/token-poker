#[cfg(windows)]
use crate::runtime_protocol::{
    parse_runtime_client_line, EventJournal, RuntimeClientRequest, RuntimeEvent,
    RuntimeServerFrame, RUNTIME_PROTOCOL_VERSION,
};
use crate::runtime_protocol::{validate_bootstrap_commands, validate_worker_args};
use anyhow::{Context, Result};
#[cfg(windows)]
use rand_core::{OsRng, RngCore};
use serde_json::Value;
#[cfg(windows)]
use std::path::Path;
use std::{env, ffi::OsString, path::PathBuf, time::Duration};

const DEFAULT_IDLE_TIMEOUT: Duration = Duration::from_secs(30 * 60);
const BOOTSTRAP_ENVIRONMENT: &str = "TOKEN_HOLDEM_BOOTSTRAP_COMMANDS";

#[derive(Debug, Clone)]
pub struct SupervisorConfig {
    pub pipe_name: String,
    pub worker_executable: PathBuf,
    pub worker_args: Vec<String>,
    pub bootstrap_commands: Vec<String>,
    pub idle_timeout: Duration,
}

impl SupervisorConfig {
    pub fn from_process() -> Result<Self> {
        let executable = env::current_exe().context("无法定位运行时监督器")?;
        let default_worker =
            executable
                .parent()
                .context("运行时监督器缺少父目录")?
                .join(if cfg!(windows) {
                    "token-holdem-sidecar.exe"
                } else {
                    "token-holdem-sidecar"
                });
        let bootstrap_values: Vec<Value> = match env::var(BOOTSTRAP_ENVIRONMENT) {
            Ok(value) => serde_json::from_str(&value).context("运行时引导命令环境变量无效")?,
            Err(env::VarError::NotPresent) => Vec::new(),
            Err(error) => return Err(error).context("无法读取运行时引导命令环境变量"),
        };
        let bootstrap_commands = validate_bootstrap_commands(bootstrap_values)?;
        parse_supervisor_args(env::args_os().skip(1), default_worker, bootstrap_commands)
    }
}

fn parse_supervisor_args(
    arguments: impl Iterator<Item = OsString>,
    default_worker: PathBuf,
    bootstrap_commands: Vec<String>,
) -> Result<SupervisorConfig> {
    let mut pipe_name = None;
    let mut worker_executable = default_worker;
    let mut idle_timeout = DEFAULT_IDLE_TIMEOUT;
    let mut worker_args = Vec::new();
    let mut parsing_worker_args = false;
    for argument in arguments {
        if parsing_worker_args {
            worker_args.push(
                argument
                    .into_string()
                    .map_err(|_| anyhow::anyhow!("牌局内核启动参数必须是 Unicode"))?,
            );
            continue;
        }
        if argument == "--" {
            parsing_worker_args = true;
            continue;
        }
        let value = argument.to_string_lossy();
        if let Some(raw) = value.strip_prefix("--pipe-name=") {
            if raw.is_empty() || pipe_name.replace(raw.to_owned()).is_some() {
                anyhow::bail!("--pipe-name 参数无效或重复")
            }
            continue;
        }
        if let Some(raw) = value.strip_prefix("--worker-executable=") {
            if raw.is_empty() {
                anyhow::bail!("--worker-executable 参数不能为空")
            }
            worker_executable = PathBuf::from(raw);
            continue;
        }
        if let Some(raw) = value.strip_prefix("--idle-timeout-seconds=") {
            let seconds = raw
                .parse::<u64>()
                .context("--idle-timeout-seconds 必须是整数")?;
            if !(1..=24 * 60 * 60).contains(&seconds) {
                anyhow::bail!("--idle-timeout-seconds 必须在 1 到 86400 之间")
            }
            idle_timeout = Duration::from_secs(seconds);
            continue;
        }
        anyhow::bail!("未知运行时参数：{value}")
    }
    let pipe_name = pipe_name.context("缺少 --pipe-name")?;
    validate_pipe_name(&pipe_name)?;
    validate_worker_args(&worker_args)?;
    Ok(SupervisorConfig {
        pipe_name,
        worker_executable,
        worker_args,
        bootstrap_commands,
        idle_timeout,
    })
}

fn validate_pipe_name(pipe_name: &str) -> Result<()> {
    const PREFIX: &str = r"\\.\pipe\token-holdem-runtime-v7-";
    let Some(suffix) = pipe_name.strip_prefix(PREFIX) else {
        anyhow::bail!("运行时命名管道前缀无效")
    };
    if suffix.len() < 12
        || suffix.len() > 64
        || !suffix.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        anyhow::bail!("运行时命名管道后缀必须是 12 到 64 位十六进制文本")
    }
    Ok(())
}

pub async fn run(config: SupervisorConfig) -> Result<()> {
    #[cfg(windows)]
    {
        windows::run(config).await
    }
    #[cfg(not(windows))]
    {
        let _ = config;
        anyhow::bail!("Token Poker 共享运行时目前只支持 Windows")
    }
}

#[cfg(windows)]
fn create_runtime_id() -> String {
    let mut bytes = [0_u8; 16];
    OsRng.fill_bytes(&mut bytes);
    hex::encode(bytes)
}

#[cfg(windows)]
mod windows {
    use super::*;
    use futures::{SinkExt, StreamExt};
    use std::{
        collections::VecDeque,
        process::Stdio,
        sync::{
            atomic::{AtomicU64, AtomicUsize, Ordering},
            Arc,
        },
        time::{SystemTime, UNIX_EPOCH},
    };
    use tokio::{
        io::AsyncWriteExt,
        net::windows::named_pipe::{NamedPipeServer, ServerOptions},
        process::{Child, ChildStdin, Command},
        sync::{broadcast, mpsc, oneshot, watch, RwLock},
        time::{interval, timeout, MissedTickBehavior},
    };
    use tokio_util::codec::{FramedRead, FramedWrite, LinesCodec};

    const PIPE_INSTANCES: usize = 32;
    const EVENT_BROADCAST_CAPACITY: usize = 4_096;
    const ACTOR_CHANNEL_CAPACITY: usize = 256;
    const WORKER_EVENT_CHANNEL_CAPACITY: usize = 1_024;
    const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
    const WORKER_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);
    const WORKER_STDERR_TAIL_LINES: usize = 8;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    #[derive(Debug, Default)]
    struct RuntimeStatus {
        worker_pid: Option<u32>,
    }

    enum ActorRequest {
        Ensure {
            response: oneshot::Sender<Result<(), String>>,
        },
        Forward {
            encoded: String,
            response: oneshot::Sender<Result<(), String>>,
        },
        Restart {
            worker_args: Vec<String>,
            bootstrap_commands: Vec<String>,
            response: oneshot::Sender<Result<(), String>>,
        },
        Shutdown {
            response: oneshot::Sender<Result<(), String>>,
        },
    }

    enum WorkerOutput {
        Event {
            generation: u64,
            line: String,
        },
        Closed {
            generation: u64,
            read_error: Option<String>,
        },
    }

    struct ManagedWorker {
        child: Child,
        stdin: ChildStdin,
        generation: u64,
        bootstrapped: bool,
        stderr_tail: Arc<RwLock<VecDeque<String>>>,
    }

    struct ClientLease {
        client_count: Arc<AtomicUsize>,
        last_detached_unix_ms: Arc<AtomicU64>,
    }

    impl ClientLease {
        fn acquire(client_count: Arc<AtomicUsize>, last_detached_unix_ms: Arc<AtomicU64>) -> Self {
            client_count.fetch_add(1, Ordering::AcqRel);
            Self {
                client_count,
                last_detached_unix_ms,
            }
        }
    }

    impl Drop for ClientLease {
        fn drop(&mut self) {
            let previous = self.client_count.fetch_sub(1, Ordering::AcqRel);
            if previous <= 1 {
                self.last_detached_unix_ms
                    .store(unix_time_ms(), Ordering::Release);
            }
        }
    }

    pub(super) async fn run(config: SupervisorConfig) -> Result<()> {
        let first_server = create_pipe_server(&config.pipe_name, true)
            .with_context(|| format!("无法创建单实例命名管道：{}", config.pipe_name))?;
        let journal = Arc::new(RwLock::new(EventJournal::new(create_runtime_id())));
        let status = Arc::new(RwLock::new(RuntimeStatus::default()));
        let (event_sender, _) = broadcast::channel(EVENT_BROADCAST_CAPACITY);
        let (request_sender, request_receiver) = mpsc::channel(ACTOR_CHANNEL_CAPACITY);
        let (worker_output_sender, worker_output_receiver) =
            mpsc::channel(WORKER_EVENT_CHANNEL_CAPACITY);
        let (shutdown_sender, shutdown_receiver) = watch::channel(false);
        let client_count = Arc::new(AtomicUsize::new(0));
        let last_detached_unix_ms = Arc::new(AtomicU64::new(unix_time_ms()));

        tokio::spawn(runtime_actor(
            config.clone(),
            Arc::clone(&journal),
            Arc::clone(&status),
            event_sender.clone(),
            request_receiver,
            worker_output_receiver,
            worker_output_sender,
            shutdown_sender.clone(),
        ));
        tokio::spawn(idle_monitor(
            config.idle_timeout,
            Arc::clone(&journal),
            Arc::clone(&client_count),
            Arc::clone(&last_detached_unix_ms),
            request_sender.clone(),
            shutdown_receiver.clone(),
        ));

        accept_clients(
            first_server,
            config.pipe_name,
            journal,
            status,
            event_sender,
            request_sender,
            client_count,
            last_detached_unix_ms,
            shutdown_receiver,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn accept_clients(
        mut server: NamedPipeServer,
        pipe_name: String,
        journal: Arc<RwLock<EventJournal>>,
        status: Arc<RwLock<RuntimeStatus>>,
        event_sender: broadcast::Sender<RuntimeEvent>,
        request_sender: mpsc::Sender<ActorRequest>,
        client_count: Arc<AtomicUsize>,
        last_detached_unix_ms: Arc<AtomicU64>,
        mut shutdown: watch::Receiver<bool>,
    ) -> Result<()> {
        loop {
            tokio::select! {
                connected = server.connect() => {
                    connected.context("等待命名管道客户端失败")?;
                    let connected_server = server;
                    server = create_pipe_server(&pipe_name, false)
                        .context("创建下一命名管道实例失败")?;
                    tokio::spawn(handle_client(
                        connected_server,
                        Arc::clone(&journal),
                        Arc::clone(&status),
                        event_sender.clone(),
                        request_sender.clone(),
                        Arc::clone(&client_count),
                        Arc::clone(&last_detached_unix_ms),
                        shutdown.clone(),
                    ));
                }
                changed = shutdown.changed() => {
                    if changed.is_err() || *shutdown.borrow() {
                        return Ok(());
                    }
                }
                signal = tokio::signal::ctrl_c() => {
                    signal.context("监听运行时退出信号失败")?;
                    let _ = request_actor(&request_sender, |response| ActorRequest::Shutdown { response }).await;
                    return Ok(());
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn handle_client(
        pipe: NamedPipeServer,
        journal: Arc<RwLock<EventJournal>>,
        status: Arc<RwLock<RuntimeStatus>>,
        event_sender: broadcast::Sender<RuntimeEvent>,
        request_sender: mpsc::Sender<ActorRequest>,
        client_count: Arc<AtomicUsize>,
        last_detached_unix_ms: Arc<AtomicU64>,
        mut shutdown: watch::Receiver<bool>,
    ) {
        if let Err(error) = handle_client_inner(
            pipe,
            journal,
            status,
            event_sender,
            request_sender,
            client_count,
            last_detached_unix_ms,
            &mut shutdown,
        )
        .await
        {
            eprintln!("[token-holdem-runtime] 客户端连接结束：{error:#}");
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn handle_client_inner(
        pipe: NamedPipeServer,
        journal: Arc<RwLock<EventJournal>>,
        status: Arc<RwLock<RuntimeStatus>>,
        event_sender: broadcast::Sender<RuntimeEvent>,
        request_sender: mpsc::Sender<ActorRequest>,
        client_count: Arc<AtomicUsize>,
        last_detached_unix_ms: Arc<AtomicU64>,
        shutdown: &mut watch::Receiver<bool>,
    ) -> Result<()> {
        let (read_half, write_half) = tokio::io::split(pipe);
        let mut lines = FramedRead::new(
            read_half,
            LinesCodec::new_with_max_length(crate::MAX_COMMAND_LINE_BYTES),
        );
        let mut writer = FramedWrite::new(write_half, LinesCodec::new());
        let first_line = timeout(HANDSHAKE_TIMEOUT, lines.next())
            .await
            .context("命名管道握手超时")?
            .context("客户端在握手前断开")?
            .context("读取命名管道握手失败")?;
        match parse_runtime_client_line(&first_line) {
            Ok(RuntimeClientRequest::Attach) => {}
            Ok(_) => {
                send_frame(
                    &mut writer,
                    RuntimeServerFrame::RuntimeError {
                        code: "attach_required",
                        message: "第一条运行时消息必须是 runtime_attach".to_owned(),
                    },
                )
                .await?;
                return Ok(());
            }
            Err(error) => {
                send_frame(
                    &mut writer,
                    RuntimeServerFrame::RuntimeError {
                        code: "invalid_attach",
                        message: error.to_string(),
                    },
                )
                .await?;
                return Ok(());
            }
        }

        let _lease = ClientLease::acquire(client_count, last_detached_unix_ms);
        let mut live_events = event_sender.subscribe();
        request_actor(&request_sender, |response| ActorRequest::Ensure {
            response,
        })
        .await
        .map_err(anyhow::Error::msg)?;
        let snapshot = journal.read().await.snapshot();
        let worker_pid = status.read().await.worker_pid;
        send_frame(
            &mut writer,
            RuntimeServerFrame::RuntimeAttached {
                protocol_version: RUNTIME_PROTOCOL_VERSION,
                runtime_id: snapshot.runtime_id.clone(),
                worker_pid,
                generation: snapshot.generation,
                latest_sequence: snapshot.latest_sequence,
                earliest_sequence: snapshot.earliest_sequence,
                history_truncated: snapshot.history_truncated,
            },
        )
        .await?;
        for event in snapshot.events {
            send_frame(&mut writer, event.into()).await?;
        }
        send_frame(
            &mut writer,
            RuntimeServerFrame::RuntimeReplayComplete {
                generation: snapshot.generation,
                latest_sequence: snapshot.latest_sequence,
            },
        )
        .await?;
        let mut delivered_sequence = snapshot.latest_sequence;

        loop {
            tokio::select! {
                line = lines.next() => {
                    let Some(line) = line else { return Ok(()); };
                    let line = line.context("读取运行时客户端消息失败")?;
                    match parse_runtime_client_line(&line) {
                        Ok(RuntimeClientRequest::ForwardWorkerCommand { encoded }) => {
                            send_actor_result(
                                &mut writer,
                                request_actor(&request_sender, |response| ActorRequest::Forward { encoded, response }).await,
                            ).await?;
                        }
                        Ok(RuntimeClientRequest::Restart { worker_args, bootstrap_commands }) => {
                            send_actor_result(
                                &mut writer,
                                request_actor(&request_sender, |response| ActorRequest::Restart {
                                    worker_args,
                                    bootstrap_commands,
                                    response,
                                }).await,
                            ).await?;
                        }
                        Ok(RuntimeClientRequest::ShutdownRuntime) => {
                            send_actor_result(
                                &mut writer,
                                request_actor(&request_sender, |response| ActorRequest::Shutdown { response }).await,
                            ).await?;
                            return Ok(());
                        }
                        Ok(RuntimeClientRequest::Attach) => {
                            send_frame(&mut writer, RuntimeServerFrame::RuntimeError {
                                code: "already_attached",
                                message: "当前连接已经完成运行时握手".to_owned(),
                            }).await?;
                        }
                        Err(error) => {
                            send_frame(&mut writer, RuntimeServerFrame::RuntimeError {
                                code: "invalid_request",
                                message: error.to_string(),
                            }).await?;
                        }
                    }
                }
                event = live_events.recv() => {
                    match event {
                        Ok(event) if event.sequence > delivered_sequence => {
                            delivered_sequence = event.sequence;
                            send_frame(&mut writer, event.into()).await?;
                        }
                        Ok(_) => {}
                        Err(broadcast::error::RecvError::Lagged(_)) => {
                            send_frame(&mut writer, RuntimeServerFrame::RuntimeError {
                                code: "event_stream_lagged",
                                message: "客户端处理事件过慢，请重新连接并回放状态".to_owned(),
                            }).await?;
                            return Ok(());
                        }
                        Err(broadcast::error::RecvError::Closed) => return Ok(()),
                    }
                }
                changed = shutdown.changed() => {
                    if changed.is_err() || *shutdown.borrow() {
                        return Ok(());
                    }
                }
            }
        }
    }

    async fn send_actor_result<W>(
        writer: &mut FramedWrite<W, LinesCodec>,
        result: Result<(), String>,
    ) -> Result<()>
    where
        W: tokio::io::AsyncWrite + Unpin,
    {
        if let Err(message) = result {
            send_frame(
                writer,
                RuntimeServerFrame::RuntimeError {
                    code: "runtime_operation_failed",
                    message,
                },
            )
            .await?;
        }
        Ok(())
    }

    async fn send_frame<W>(
        writer: &mut FramedWrite<W, LinesCodec>,
        frame: RuntimeServerFrame,
    ) -> Result<()>
    where
        W: tokio::io::AsyncWrite + Unpin,
    {
        writer
            .send(serde_json::to_string(&frame).context("序列化运行时帧失败")?)
            .await
            .context("写入运行时帧失败")
    }

    async fn request_actor(
        sender: &mpsc::Sender<ActorRequest>,
        create: impl FnOnce(oneshot::Sender<Result<(), String>>) -> ActorRequest,
    ) -> Result<(), String> {
        let (response_sender, response_receiver) = oneshot::channel();
        sender
            .send(create(response_sender))
            .await
            .map_err(|_| "运行时内核监督器已经退出".to_owned())?;
        response_receiver
            .await
            .map_err(|_| "运行时内核监督器未返回结果".to_owned())?
    }

    #[allow(clippy::too_many_arguments)]
    async fn runtime_actor(
        mut config: SupervisorConfig,
        journal: Arc<RwLock<EventJournal>>,
        status: Arc<RwLock<RuntimeStatus>>,
        event_sender: broadcast::Sender<RuntimeEvent>,
        mut requests: mpsc::Receiver<ActorRequest>,
        mut worker_outputs: mpsc::Receiver<WorkerOutput>,
        worker_output_sender: mpsc::Sender<WorkerOutput>,
        shutdown_sender: watch::Sender<bool>,
    ) {
        let generation = current_generation(&journal).await;
        let mut worker = match spawn_worker(&config, generation, worker_output_sender.clone()).await
        {
            Ok(worker) => Some(worker),
            Err(error) => {
                publish_warning(
                    &journal,
                    &event_sender,
                    &format!("无法启动牌局内核：{error:#}；下一次操作会重试。"),
                )
                .await;
                None
            }
        };
        update_worker_status(&status, worker.as_ref()).await;

        loop {
            tokio::select! {
                request = requests.recv() => {
                    let Some(request) = request else { break; };
                    match request {
                        ActorRequest::Ensure { response } => {
                            let result = ensure_worker(
                                &config,
                                &journal,
                                &status,
                                &worker_output_sender,
                                &mut worker,
                            ).await;
                            let _ = response.send(result.map_err(|error| format!("{error:#}")));
                        }
                        ActorRequest::Forward { encoded, response } => {
                            let result = async {
                                ensure_worker(
                                    &config,
                                    &journal,
                                    &status,
                                    &worker_output_sender,
                                    &mut worker,
                                ).await?;
                                write_worker_line(worker.as_mut().context("牌局内核尚未启动")?, &encoded).await
                            }.await;
                            let _ = response.send(result.map_err(|error| format!("{error:#}")));
                        }
                        ActorRequest::Restart { worker_args, bootstrap_commands, response } => {
                            let result = async {
                                {
                                    let mut journal = journal.write().await;
                                    journal.reset_generation();
                                }
                                publish_event(
                                    &journal,
                                    &event_sender,
                                    serde_json::json!({"type": "sidecar_restarting"}),
                                ).await?;
                                let stop_result = if let Some(existing) = worker.as_mut() {
                                    stop_worker(existing).await
                                } else {
                                    Ok(())
                                };
                                drop(worker.take());
                                update_worker_status(&status, None).await;
                                stop_result?;
                                config.worker_args = worker_args;
                                config.bootstrap_commands = bootstrap_commands;
                                ensure_worker(
                                    &config,
                                    &journal,
                                    &status,
                                    &worker_output_sender,
                                    &mut worker,
                                ).await
                            }.await;
                            let _ = response.send(result.map_err(|error| format!("{error:#}")));
                        }
                        ActorRequest::Shutdown { response } => {
                            let result = if let Some(existing) = worker.as_mut() {
                                stop_worker(existing).await
                            } else {
                                Ok(())
                            };
                            drop(worker.take());
                            update_worker_status(&status, None).await;
                            let _ = response.send(result.map_err(|error| format!("{error:#}")));
                            break;
                        }
                    }
                }
                output = worker_outputs.recv() => {
                    let Some(output) = output else { break; };
                    match output {
                        WorkerOutput::Event { generation, line } => {
                            let active_generation = current_generation(&journal).await;
                            if generation != active_generation {
                                continue;
                            }
                            match serde_json::from_str::<Value>(&line) {
                                Ok(event) => {
                                    let ready = event.get("type").and_then(Value::as_str) == Some("ready");
                                    if let Err(error) = publish_event(&journal, &event_sender, event).await {
                                        publish_warning(&journal, &event_sender, &format!("牌局内核事件无效：{error:#}")).await;
                                    }
                                    if ready {
                                        if let Some(active_worker) = worker.as_mut() {
                                            if active_worker.generation == generation && !active_worker.bootstrapped {
                                                let bootstrap_result = write_bootstrap_commands(active_worker, &config.bootstrap_commands).await;
                                                active_worker.bootstrapped = bootstrap_result.is_ok();
                                                if let Err(error) = bootstrap_result {
                                                    publish_warning(&journal, &event_sender, &format!("社区网络引导失败：{error:#}")).await;
                                                }
                                            }
                                        }
                                    }
                                }
                                Err(error) => {
                                    publish_warning(&journal, &event_sender, &format!("牌局内核输出了无效 JSON：{error}")).await;
                                }
                            }
                        }
                        WorkerOutput::Closed { generation, read_error } => {
                            let is_current = worker.as_ref().is_some_and(|value| value.generation == generation);
                            if !is_current {
                                continue;
                            }
                            if let Some(mut closed) = worker.take() {
                                update_worker_status(&status, None).await;
                                journal.write().await.reset_generation();
                                let exit = inspect_unexpected_worker_exit(&mut closed, read_error).await;
                                let message = format!(
                                    "牌局内核意外退出（{exit}）；运行状态已失效，界面将重新初始化。"
                                );
                                publish_warning(&journal, &event_sender, &message).await;
                            }
                        }
                    }
                }
            }
        }
        let _ = shutdown_sender.send(true);
    }

    async fn ensure_worker(
        config: &SupervisorConfig,
        journal: &Arc<RwLock<EventJournal>>,
        status: &Arc<RwLock<RuntimeStatus>>,
        worker_output_sender: &mpsc::Sender<WorkerOutput>,
        worker: &mut Option<ManagedWorker>,
    ) -> Result<()> {
        if worker.is_none() {
            let generation = current_generation(journal).await;
            *worker = Some(spawn_worker(config, generation, worker_output_sender.clone()).await?);
            update_worker_status(status, worker.as_ref()).await;
        }
        Ok(())
    }

    async fn spawn_worker(
        config: &SupervisorConfig,
        generation: u64,
        output_sender: mpsc::Sender<WorkerOutput>,
    ) -> Result<ManagedWorker> {
        ensure_worker_executable(&config.worker_executable)?;
        let mut command = Command::new(&config.worker_executable);
        command
            .args(&config.worker_args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .env_remove(BOOTSTRAP_ENVIRONMENT);
        use std::os::windows::process::CommandExt as _;
        command.as_std_mut().creation_flags(CREATE_NO_WINDOW);
        let mut child = command
            .spawn()
            .with_context(|| format!("无法启动牌局内核：{}", config.worker_executable.display()))?;
        let stdin = child.stdin.take().context("牌局内核缺少标准输入")?;
        let stdout = child.stdout.take().context("牌局内核缺少标准输出")?;
        let stderr = child.stderr.take().context("牌局内核缺少标准错误")?;

        let stdout_sender = output_sender.clone();
        tokio::spawn(async move {
            let mut read_error = None;
            let mut lines = FramedRead::new(
                stdout,
                LinesCodec::new_with_max_length(crate::MAX_COMMAND_LINE_BYTES),
            );
            while let Some(line) = lines.next().await {
                match line {
                    Ok(line) => {
                        if stdout_sender
                            .send(WorkerOutput::Event { generation, line })
                            .await
                            .is_err()
                        {
                            return;
                        }
                    }
                    Err(error) => {
                        eprintln!("[token-holdem-runtime] 读取牌局事件失败：{error}");
                        read_error = Some(error.to_string());
                        break;
                    }
                }
            }
            let _ = stdout_sender
                .send(WorkerOutput::Closed {
                    generation,
                    read_error,
                })
                .await;
        });
        let stderr_tail = Arc::new(RwLock::new(VecDeque::with_capacity(
            WORKER_STDERR_TAIL_LINES,
        )));
        let captured_stderr = Arc::clone(&stderr_tail);
        tokio::spawn(async move {
            let mut lines = FramedRead::new(stderr, LinesCodec::new_with_max_length(16 * 1_024));
            while let Some(line) = lines.next().await {
                match line {
                    Ok(line) => {
                        eprintln!("[token-holdem-sidecar] {line}");
                        let mut tail = captured_stderr.write().await;
                        if tail.len() == WORKER_STDERR_TAIL_LINES {
                            tail.pop_front();
                        }
                        tail.push_back(line);
                    }
                    Err(error) => {
                        eprintln!("[token-holdem-runtime] 读取牌局日志失败：{error}");
                        return;
                    }
                }
            }
        });

        Ok(ManagedWorker {
            child,
            stdin,
            generation,
            bootstrapped: false,
            stderr_tail,
        })
    }

    fn ensure_worker_executable(path: &Path) -> Result<()> {
        let metadata = std::fs::metadata(path)
            .with_context(|| format!("牌局内核不存在：{}", path.display()))?;
        if !metadata.is_file() {
            anyhow::bail!("牌局内核路径不是文件：{}", path.display())
        }
        Ok(())
    }

    async fn write_worker_line(worker: &mut ManagedWorker, encoded: &str) -> Result<()> {
        worker
            .stdin
            .write_all(encoded.as_bytes())
            .await
            .context("写入牌局内核命令失败")?;
        worker
            .stdin
            .write_all(b"\n")
            .await
            .context("结束牌局内核命令失败")?;
        worker.stdin.flush().await.context("刷新牌局内核命令失败")
    }

    async fn write_bootstrap_commands(
        worker: &mut ManagedWorker,
        commands: &[String],
    ) -> Result<()> {
        for command in commands {
            write_worker_line(worker, command).await?;
        }
        Ok(())
    }

    async fn stop_worker(worker: &mut ManagedWorker) -> Result<()> {
        let _ = write_worker_line(worker, r#"{"type":"shutdown"}"#).await;
        match timeout(WORKER_SHUTDOWN_TIMEOUT, worker.child.wait()).await {
            Ok(result) => {
                result.context("等待牌局内核退出失败")?;
            }
            Err(_) => {
                worker.child.kill().await.context("强制停止牌局内核失败")?;
                worker.child.wait().await.context("回收牌局内核失败")?;
            }
        }
        Ok(())
    }

    async fn inspect_unexpected_worker_exit(
        worker: &mut ManagedWorker,
        read_error: Option<String>,
    ) -> String {
        let _ = worker.stdin.shutdown().await;
        let status = match timeout(WORKER_SHUTDOWN_TIMEOUT, worker.child.wait()).await {
            Ok(Ok(status)) => format!("退出状态 {status}"),
            Ok(Err(error)) => format!("无法读取退出状态：{error}"),
            Err(_) => match worker.child.kill().await {
                Ok(()) => {
                    let _ = worker.child.wait().await;
                    "退出超时，已强制回收".to_owned()
                }
                Err(error) => format!("退出超时且无法强制回收：{error}"),
            },
        };
        let stderr = worker
            .stderr_tail
            .read()
            .await
            .iter()
            .cloned()
            .collect::<Vec<_>>()
            .join(" | ");
        match (read_error, stderr.is_empty()) {
            (Some(error), false) => format!("{status}；事件读取错误：{error}；日志：{stderr}"),
            (Some(error), true) => format!("{status}；事件读取错误：{error}"),
            (None, false) => format!("{status}；日志：{stderr}"),
            (None, true) => status,
        }
    }

    async fn publish_event(
        journal: &Arc<RwLock<EventJournal>>,
        sender: &broadcast::Sender<RuntimeEvent>,
        event: Value,
    ) -> Result<()> {
        let event = journal.write().await.append(event)?;
        let _ = sender.send(event);
        Ok(())
    }

    async fn publish_warning(
        journal: &Arc<RwLock<EventJournal>>,
        sender: &broadcast::Sender<RuntimeEvent>,
        message: &str,
    ) {
        let _ = publish_event(
            journal,
            sender,
            serde_json::json!({"type": "warning", "message": message}),
        )
        .await;
    }

    async fn current_generation(journal: &Arc<RwLock<EventJournal>>) -> u64 {
        journal.read().await.snapshot().generation
    }

    async fn update_worker_status(
        status: &Arc<RwLock<RuntimeStatus>>,
        worker: Option<&ManagedWorker>,
    ) {
        status.write().await.worker_pid = worker.and_then(|value| value.child.id());
    }

    async fn idle_monitor(
        idle_timeout: Duration,
        journal: Arc<RwLock<EventJournal>>,
        client_count: Arc<AtomicUsize>,
        last_detached_unix_ms: Arc<AtomicU64>,
        request_sender: mpsc::Sender<ActorRequest>,
        mut shutdown: watch::Receiver<bool>,
    ) {
        let tick = idle_timeout
            .div_f64(10.0)
            .clamp(Duration::from_millis(250), Duration::from_secs(30));
        let mut timer = interval(tick);
        timer.set_missed_tick_behavior(MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                _ = timer.tick() => {
                    if client_count.load(Ordering::Acquire) != 0 || journal.read().await.is_busy() {
                        continue;
                    }
                    let elapsed_ms = unix_time_ms().saturating_sub(
                        last_detached_unix_ms.load(Ordering::Acquire),
                    );
                    if elapsed_ms >= idle_timeout.as_millis().min(u128::from(u64::MAX)) as u64 {
                        let _ = request_actor(&request_sender, |response| ActorRequest::Shutdown { response }).await;
                        return;
                    }
                }
                changed = shutdown.changed() => {
                    if changed.is_err() || *shutdown.borrow() {
                        return;
                    }
                }
            }
        }
    }

    fn create_pipe_server(
        pipe_name: &str,
        first_instance: bool,
    ) -> std::io::Result<NamedPipeServer> {
        let mut options = ServerOptions::new();
        options
            .first_pipe_instance(first_instance)
            .max_instances(PIPE_INSTANCES);
        options.create(pipe_name)
    }

    fn unix_time_ms() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
            .min(u128::from(u64::MAX)) as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 监督器参数要求受限命名管道并保留牌局参数() {
        let config = parse_supervisor_args(
            [
                OsString::from(r"--pipe-name=\\.\pipe\token-holdem-runtime-v7-aabbccddeeff"),
                OsString::from("--idle-timeout-seconds=30"),
                OsString::from("--"),
                OsString::from("--volunteer-consent=granted"),
            ]
            .into_iter(),
            PathBuf::from("token-holdem-sidecar.exe"),
            Vec::new(),
        )
        .expect("合法监督器参数应通过");
        assert_eq!(config.idle_timeout, Duration::from_secs(30));
        assert_eq!(config.worker_args, ["--volunteer-consent=granted"]);
    }

    #[test]
    fn 监督器拒绝任意命名管道和守护模式牌局内核() {
        assert!(parse_supervisor_args(
            [OsString::from(r"--pipe-name=\\.\pipe\other")].into_iter(),
            PathBuf::from("token-holdem-sidecar.exe"),
            Vec::new(),
        )
        .is_err());
        assert!(parse_supervisor_args(
            [
                OsString::from(r"--pipe-name=\\.\pipe\token-holdem-runtime-v7-aabbccddeeff"),
                OsString::from("--"),
                OsString::from("--daemon"),
            ]
            .into_iter(),
            PathBuf::from("token-holdem-sidecar.exe"),
            Vec::new(),
        )
        .is_err());
    }
}
