import assert from "node:assert/strict";
import vm from "node:vm";
import test from "node:test";
import { buildHostBridge } from "../src/ui-resource.mjs";

function flushTasks() {
  return new Promise((resolve) => setImmediate(resolve));
}

function createHarness(options = {}) {
  const state = {
    app: null,
    appCapabilities: null,
    appOptions: null,
    calls: [],
    cleanupCount: 0,
    dispatchedEvents: [],
    requestDisplayModeCount: 0,
    requestTeardownCount: 0,
    resizeSetupCount: 0,
    teardownRegisteredBeforeConnect: false,
  };
  const globalListeners = new Map();
  const documentListeners = new Map();
  const nodes = new Map([
    ["token-holdem-root", { id: "token-holdem-root" }],
    ["token-holdem-portals", { id: "token-holdem-portals" }],
    ["token-poker-boot-status", { id: "token-poker-boot-status", textContent: "Opening" }],
  ]);

  class MockApp {
    constructor(_info, capabilities, appOptions) {
      this.listeners = new Map();
      this.hostContext = {
        displayMode: options.initialDisplayMode ?? "fullscreen",
        theme: "light",
      };
      state.app = this;
      state.appCapabilities = capabilities;
      state.appOptions = appOptions;
    }

    addEventListener(type, listener) {
      const listeners = this.listeners.get(type) ?? new Set();
      listeners.add(listener);
      this.listeners.set(type, listeners);
    }

    removeEventListener(type, listener) {
      this.listeners.get(type)?.delete(listener);
    }

    emit(type, detail) {
      if (type === "hostcontextchanged") {
        this.hostContext = { ...this.hostContext, ...detail };
      }
      for (const listener of this.listeners.get(type) ?? []) listener(detail);
    }

    connect() {
      state.teardownRegisteredBeforeConnect = typeof this.onteardown === "function";
      return options.connectPromise ?? Promise.resolve();
    }

    getHostContext() {
      return this.hostContext;
    }

    requestDisplayMode() {
      state.requestDisplayModeCount += 1;
      return Promise.resolve({ mode: options.requestedDisplayMode ?? "fullscreen" });
    }

    setupSizeChangedNotifications() {
      state.resizeSetupCount += 1;
      if (options.resizeSetupError instanceof Error) throw options.resizeSetupError;
      let cleaned = false;
      return () => {
        if (cleaned) return;
        cleaned = true;
        state.cleanupCount += 1;
      };
    }

    callServerTool(params, requestOptions) {
      state.calls.push({ params, options: requestOptions });
      if (typeof options.callServerTool === "function") {
        return options.callServerTool(params, requestOptions, state);
      }
      if (params.name === "token_holdem_poll") {
        return new Promise((_resolve, reject) => {
          requestOptions.signal.addEventListener(
            "abort",
            () => reject(requestOptions.signal.reason),
            { once: true },
          );
        });
      }
      return Promise.resolve({ structuredContent: {} });
    }

    requestTeardown() {
      state.requestTeardownCount += 1;
      return Promise.resolve();
    }
  }

  class MockCustomEvent {
    constructor(type, init = {}) {
      this.type = type;
      this.detail = init.detail;
    }
  }

  const sandbox = {
    AbortController,
    CustomEvent: MockCustomEvent,
    clearTimeout,
    console,
    document: {
      visibilityState: options.initialVisibilityState ?? "visible",
      addEventListener(type, listener) {
        const listeners = documentListeners.get(type) ?? new Set();
        listeners.add(listener);
        documentListeners.set(type, listeners);
      },
      getElementById(id) {
        return nodes.get(id) ?? null;
      },
      removeEventListener(type, listener) {
        documentListeners.get(type)?.delete(listener);
      },
    },
    requestAnimationFrame(callback) {
      return setImmediate(() => callback(Date.now()));
    },
    setTimeout: options.setTimeout ?? setTimeout,
    __TOKEN_HOLDEM_MCP_APPS__: {
      App: MockApp,
      applyDocumentTheme() {},
      applyHostFonts() {},
      applyHostStyleVariables() {},
    },
    addEventListener(type, listener) {
      const listeners = globalListeners.get(type) ?? new Set();
      listeners.add(listener);
      globalListeners.set(type, listeners);
    },
    removeEventListener(type, listener) {
      globalListeners.get(type)?.delete(listener);
    },
    dispatchEvent(event) {
      state.dispatchedEvents.push(event);
      for (const listener of globalListeners.get(event.type) ?? []) listener(event);
    },
  };

  vm.runInNewContext(buildHostBridge("9.9.9"), sandbox, {
    filename: "token-poker-host-bridge.js",
  });

  return {
    app: state.app,
    command(payload) {
      sandbox.tokenHoldemCommand(payload);
    },
    dispatchGlobal(type) {
      for (const listener of globalListeners.get(type) ?? []) listener({ type });
    },
    dispatchDocument(type, visibilityState) {
      if (visibilityState !== undefined) sandbox.document.visibilityState = visibilityState;
      for (const listener of documentListeners.get(type) ?? []) listener({ type });
    },
    async ready() {
      await state.app.ready;
      await flushTasks();
    },
    state,
  };
}

test("桥接依赖损坏时保留可见错误，而不是白屏", () => {
  const bootStatus = { textContent: "Opening" };
  const sandbox = {
    document: {
      getElementById(id) {
        if (id === "token-holdem-root" || id === "token-holdem-portals") return { id };
        if (id === "token-poker-boot-status") return bootStatus;
        return null;
      },
    },
  };

  vm.runInNewContext(buildHostBridge("9.9.9"), sandbox);

  assert.match(sandbox.__tokenHoldemBootError, /could not initialize/u);
  assert.equal(bootStatus.textContent, sandbox.__tokenHoldemBootError);
});

test("握手完成前暂时隐藏不会被误判为永久 teardown", async () => {
  let resolveConnect;
  const connectPromise = new Promise((resolve) => {
    resolveConnect = resolve;
  });
  const harness = createHarness({ connectPromise });

  harness.dispatchGlobal("pagehide");
  resolveConnect();
  await harness.ready();

  assert.equal(harness.state.requestDisplayModeCount, 1);
  assert.equal(
    harness.state.calls.filter((entry) => entry.params.name === "token_holdem_poll").length,
    1,
  );
  const pollCall = harness.state.calls.find((entry) => entry.params.name === "token_holdem_poll");
  assert.equal(pollCall?.options.signal.aborted, false);

  await harness.app.onteardown({}, {});
});

test("pagehide 后 pageshow 会重绘而不终止 MCP 会话", async () => {
  const harness = createHarness();
  await harness.ready();
  const pollCall = harness.state.calls.find((entry) => entry.params.name === "token_holdem_poll");

  harness.dispatchGlobal("pagehide");
  await flushTasks();
  assert.equal(pollCall?.options.signal.aborted, false);

  harness.dispatchGlobal("pageshow");
  await flushTasks();
  assert.equal(
    harness.state.dispatchedEvents.filter((event) => event.type === "token-holdem:resume").length,
    1,
  );
  assert.equal(pollCall?.options.signal.aborted, false);

  await harness.app.onteardown({}, {});
});

test("visibility 恢复会重建 inline 尺寸监听并触发重绘", async () => {
  const harness = createHarness();
  await harness.ready();
  harness.app.emit("hostcontextchanged", { displayMode: "inline" });
  assert.equal(harness.state.resizeSetupCount, 1);

  harness.dispatchDocument("visibilitychange", "hidden");
  assert.equal(harness.state.cleanupCount, 1);

  harness.dispatchDocument("visibilitychange", "visible");
  await flushTasks();
  assert.equal(harness.state.resizeSetupCount, 2);
  assert.equal(
    harness.state.dispatchedEvents.filter((event) => event.type === "token-holdem:resume").length,
    1,
  );

  await harness.app.onteardown({}, {});
});

test("异步连接失败会通知 React 错误边界", async () => {
  const harness = createHarness({ connectPromise: Promise.reject(new Error("host unavailable")) });
  await harness.ready();

  const fatalEvent = harness.state.dispatchedEvents.find(
    (event) => event.type === "token-holdem:fatal",
  );
  assert.match(fatalEvent?.detail, /host unavailable/u);
});

test("宿主桥接在 connect 前注册 teardown，并禁用全屏自动测高", async () => {
  const harness = createHarness();
  await harness.ready();

  assert.equal(harness.state.teardownRegisteredBeforeConnect, true);
  assert.equal(harness.state.appOptions.autoResize, false);
  assert.deepEqual(
    [...harness.state.appCapabilities.availableDisplayModes],
    ["inline", "fullscreen"],
  );
  assert.equal(harness.state.resizeSetupCount, 0);

  await harness.app.onteardown({}, {});
});

test("尺寸通知只在 inline 模式存活，主题通知不会误停监听", async () => {
  const harness = createHarness();
  await harness.ready();

  harness.app.emit("hostcontextchanged", { displayMode: "inline" });
  assert.equal(harness.state.resizeSetupCount, 1);
  assert.equal(harness.state.cleanupCount, 0);

  harness.app.emit("hostcontextchanged", { theme: "dark" });
  assert.equal(harness.state.resizeSetupCount, 1);
  assert.equal(harness.state.cleanupCount, 0);

  harness.app.emit("hostcontextchanged", { displayMode: "fullscreen" });
  assert.equal(harness.state.cleanupCount, 1);

  await harness.app.onteardown({}, {});
});

test("inline 尺寸监听初始化失败只报告一次且不阻断会话", async () => {
  const harness = createHarness({ resizeSetupError: new Error("observer unavailable") });
  await harness.ready();

  harness.app.emit("hostcontextchanged", { displayMode: "inline" });
  harness.app.emit("hostcontextchanged", { theme: "dark" });

  assert.equal(harness.state.resizeSetupCount, 1);
  assert.equal(
    harness.state.dispatchedEvents.filter(
      (event) => event.type === "token-holdem:sidecar" && event.detail?.type === "warning",
    ).length,
    1,
  );
  await harness.app.onteardown({}, {});
});

test("宿主 teardown 会取消悬挂轮询且不会创建下一次轮询", async () => {
  let pollSignal;
  const harness = createHarness({
    callServerTool(params, requestOptions) {
      if (params.name !== "token_holdem_poll") {
        return Promise.resolve({ structuredContent: {} });
      }
      pollSignal = requestOptions.signal;
      return new Promise((_resolve, reject) => {
        pollSignal.addEventListener("abort", () => reject(pollSignal.reason), { once: true });
      });
    },
  });
  await harness.ready();
  assert.ok(pollSignal instanceof AbortSignal);

  await harness.app.onteardown({}, {});
  await flushTasks();

  assert.equal(pollSignal.aborted, true);
  assert.equal(
    harness.state.calls.filter((entry) => entry.params.name === "token_holdem_poll").length,
    1,
  );
});

test("thread not found 是终止态，不会形成僵尸重试", async () => {
  const harness = createHarness({
    callServerTool(params) {
      if (params.name === "token_holdem_poll") {
        return Promise.resolve({
          isError: true,
          content: [{ type: "text", text: "thread not found: orphaned-task" }],
        });
      }
      return Promise.resolve({ structuredContent: {} });
    },
  });
  await harness.ready();
  await flushTasks();

  assert.equal(
    harness.state.calls.filter((entry) => entry.params.name === "token_holdem_poll").length,
    1,
  );
  harness.command(JSON.stringify({ type: "close_ui" }));
  await flushTasks();
  assert.equal(harness.state.requestTeardownCount, 1);
});

test("工具超时会真实 abort 底层 MCP 调用", async () => {
  let pollSignal;
  const acceleratedSetTimeout = (callback, delay, ...args) =>
    setTimeout(callback, delay >= 30_000 ? 0 : delay, ...args);
  const harness = createHarness({
    setTimeout: acceleratedSetTimeout,
    callServerTool(params, requestOptions) {
      if (params.name !== "token_holdem_poll") {
        return Promise.resolve({ structuredContent: {} });
      }
      pollSignal = requestOptions.signal;
      return new Promise(() => undefined);
    },
  });
  await harness.ready();
  await new Promise((resolve) => setTimeout(resolve, 20));

  assert.ok(pollSignal instanceof AbortSignal);
  assert.equal(pollSignal.aborted, true);
  assert.equal(pollSignal.reason?.name, "TimeoutError");

  await harness.app.onteardown({}, {});
});
