from orders_service import BaseRepository


def open_report_session() -> str:
    repo = BaseRepository("report-dsn")
    return repo.open_session()
