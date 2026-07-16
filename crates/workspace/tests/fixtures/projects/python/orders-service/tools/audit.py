import orders_service


def audit_session() -> str:
    repo = orders_service.BaseRepository("audit-dsn")
    return repo.open_session()
