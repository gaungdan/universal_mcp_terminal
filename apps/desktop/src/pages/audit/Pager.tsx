export type PagerProps = {
  page: number;
  total: number;
  pageSize: number;
  onPageChange: (page: number) => void;
};

export default function Pager({ page, total, pageSize, onPageChange }: PagerProps) {
  const pageCount = Math.max(1, Math.ceil(total / pageSize));

  return (
    <div className="audit-pager">
      <span>
        共 {total} 条 · 第 {page + 1} / {pageCount} 页
      </span>
      <div className="audit-pager__actions">
        <button
          type="button"
          className="btn btn--ghost btn--small"
          disabled={page <= 0}
          onClick={() => onPageChange(page - 1)}
        >
          上一页
        </button>
        <button
          type="button"
          className="btn btn--ghost btn--small"
          disabled={page + 1 >= pageCount}
          onClick={() => onPageChange(page + 1)}
        >
          下一页
        </button>
      </div>
    </div>
  );
}
