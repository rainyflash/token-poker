import { Component, type ErrorInfo, type ReactNode } from "react";

interface RootErrorBoundaryProps {
  readonly children: ReactNode;
  readonly initialError?: string;
}

interface RootErrorBoundaryState {
  readonly error: string | null;
}

function prefersChinese(): boolean {
  try {
    return globalThis.navigator.languages.some((language) => language.toLowerCase().startsWith("zh"));
  } catch {
    return false;
  }
}

export class RootErrorBoundary extends Component<
  RootErrorBoundaryProps,
  RootErrorBoundaryState
> {
  public override state: RootErrorBoundaryState = {
    error: this.props.initialError ?? null,
  };

  public static getDerivedStateFromError(error: unknown): RootErrorBoundaryState {
    return {
      error: error instanceof Error ? error.message : String(error),
    };
  }

  private readonly handleFatalBridgeError = (event: Event): void => {
    const detail = event instanceof CustomEvent
      ? (event as CustomEvent<unknown>).detail
      : null;
    const message = typeof detail === "string" ? detail : null;
    if (message !== null && message.length > 0) this.setState({ error: message });
  };

  public override componentDidMount(): void {
    globalThis.addEventListener("token-holdem:fatal", this.handleFatalBridgeError);
  }

  public override componentDidCatch(error: Error, info: ErrorInfo): void {
    console.error("Token Poker UI crashed", error, info.componentStack);
  }

  public override componentWillUnmount(): void {
    globalThis.removeEventListener("token-holdem:fatal", this.handleFatalBridgeError);
  }

  public override render(): ReactNode {
    if (this.state.error === null) return this.props.children;
    const chinese = prefersChinese();
    return (
      <main className="token-poker-fatal" role="alert">
        <section className="token-poker-fatal-card">
          <span className="token-poker-fatal-kicker">TOKEN POKER</span>
          <h1>{chinese ? "界面未能完成加载" : "The table could not finish loading"}</h1>
          <p>
            {chinese
              ? "本次会话已经进入安全兜底状态。返回 Codex 后重新打开；若问题持续，请运行安装器的修复模式。"
              : "This session entered a safe fallback. Return to Codex and reopen it; run the installer repair if the problem persists."}
          </p>
          <code>{this.state.error}</code>
          <button
            type="button"
            onClick={() => {
              void globalThis.tokenHoldemCommand?.(JSON.stringify({ type: "close_ui" }));
            }}
          >
            {chinese ? "返回 Codex" : "Return to Codex"}
          </button>
        </section>
      </main>
    );
  }
}
