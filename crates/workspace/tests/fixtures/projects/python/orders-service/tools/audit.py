import orders_service


def audit_session() -> str:
	repo = orders_service.BaseRepository("audit-dsn")
	return repo.open_session()


def audit_exported_client() -> str:
    client = orders_service.ExportedClient("audit")
    return client.label()
