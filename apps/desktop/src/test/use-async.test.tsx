import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { useAsync } from "../lib/useAsync";

type ProbeProps = {
  loader: () => Promise<string>;
  dep: string;
};

function Probe({ loader, dep }: ProbeProps) {
  const { data, loading, error, reload } = useAsync(loader, [dep]);

  return (
    <div>
      <span data-testid="loading">{loading ? "loading" : "idle"}</span>
      <span data-testid="data">{data ?? "none"}</span>
      <span data-testid="error">{error ? String(error) : "none"}</span>
      <button type="button" onClick={reload}>
        reload
      </button>
    </div>
  );
}

describe("useAsync", () => {
  it("加载完成后返回数据", async () => {
    render(<Probe loader={() => Promise.resolve("done")} dep="a" />);

    expect(screen.getByTestId("loading")).toHaveTextContent("loading");

    await waitFor(() => expect(screen.getByTestId("data")).toHaveTextContent("done"));
    expect(screen.getByTestId("loading")).toHaveTextContent("idle");
    expect(screen.getByTestId("error")).toHaveTextContent("none");
  });

  it("失败时暴露错误，reload 重新请求", async () => {
    const loader = vi
      .fn<() => Promise<string>>()
      .mockRejectedValueOnce(new Error("boom"))
      .mockResolvedValue("ok");

    render(<Probe loader={loader} dep="a" />);

    await waitFor(() =>
      expect(screen.getByTestId("error")).toHaveTextContent("boom"),
    );

    fireEvent.click(screen.getByRole("button", { name: "reload" }));

    await waitFor(() => expect(screen.getByTestId("data")).toHaveTextContent("ok"));
    expect(screen.getByTestId("error")).toHaveTextContent("none");
    expect(loader).toHaveBeenCalledTimes(2);
  });
});
