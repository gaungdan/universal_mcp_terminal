import { Component, type ErrorInfo, type ReactNode } from "react";
import { HashRouter, Route, Routes } from "react-router-dom";
import AppLayout from "./layout/AppLayout";
import HomePage from "./pages/home/HomePage";
import ConnectionsPage from "./pages/connections";
import AccessControlPage from "./pages/access-control/AccessControlPage";
import AuditPage from "./pages/audit";
import SettingsPage from "./pages/settings";

type ErrorBoundaryProps = {
  children: ReactNode;
};

type ErrorBoundaryState = {
  error: Error | null;
};

class AppErrorBoundary extends Component<ErrorBoundaryProps, ErrorBoundaryState> {
  state: ErrorBoundaryState = { error: null };

  static getDerivedStateFromError(error: Error): ErrorBoundaryState {
    return { error };
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    console.error("页面渲染失败", error, info.componentStack);
  }

  render() {
    if (this.state.error) {
      return (
        <section className="page">
          <h1>页面渲染失败</h1>
          <p role="alert">{this.state.error.message}</p>
          <button
            type="button"
            className="btn btn--ghost"
            onClick={() => this.setState({ error: null })}
          >
            重试
          </button>
        </section>
      );
    }
    return this.props.children;
  }
}

export default function App() {
  return (
    <HashRouter>
      <AppErrorBoundary>
        <Routes>
          <Route element={<AppLayout />}>
            <Route index element={<HomePage />} />
            <Route path="connections" element={<ConnectionsPage />} />
            <Route path="access-control" element={<AccessControlPage />} />
            <Route path="audit" element={<AuditPage />} />
            <Route path="settings" element={<SettingsPage />} />
          </Route>
        </Routes>
      </AppErrorBoundary>
    </HashRouter>
  );
}
