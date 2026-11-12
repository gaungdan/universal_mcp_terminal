import { HashRouter, Route, Routes } from "react-router-dom";
import AppLayout from "./layout/AppLayout";

type PlaceholderPageProps = {
  title: string;
  description: string;
};

function PlaceholderPage({ title, description }: PlaceholderPageProps) {
  return (
    <section className="page">
      <h1>{title}</h1>
      <p>{description}</p>
    </section>
  );
}

export default function App() {
  return (
    <HashRouter>
      <Routes>
        <Route element={<AppLayout />}>
          <Route
            index
            element={
              <PlaceholderPage
                title="首页"
                description="工作台概览：连接状态与最近活动将在此展示。"
              />
            }
          />
          <Route
            path="connections"
            element={
              <PlaceholderPage
                title="连接管理"
                description="在此配置和管理 MCP 服务连接。"
              />
            }
          />
          <Route
            path="access-control"
            element={
              <PlaceholderPage
                title="访问控制"
                description="在此配置角色、权限与访问策略。"
              />
            }
          />
          <Route
            path="audit"
            element={
              <PlaceholderPage
                title="审计日志"
                description="在此查看系统操作与访问审计记录。"
              />
            }
          />
          <Route
            path="settings"
            element={
              <PlaceholderPage
                title="系统设置"
                description="在此调整应用外观、存储与运行参数。"
              />
            }
          />
        </Route>
      </Routes>
    </HashRouter>
  );
}
