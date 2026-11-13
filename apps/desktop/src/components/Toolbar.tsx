import type { ReactNode } from "react";

export type ToolbarProps = {
  children?: ReactNode;
  actions?: ReactNode;
};

export default function Toolbar({ children, actions }: ToolbarProps) {
  return (
    <div className="toolbar">
      <div className="toolbar__main">{children}</div>
      {actions ? <div className="toolbar__actions">{actions}</div> : null}
    </div>
  );
}
