from __future__ import annotations

import asyncio
from contextlib import asynccontextmanager
from dataclasses import dataclass
from typing import AsyncIterator, Protocol


@dataclass
# cm: def Job
class Job:
    id: str
    payload: dict[str, str]


# cm: def JobStore
class JobStore(Protocol):
    async def reserve(self) -> Job | None: ...
    async def complete(self, job_id: str) -> None: ...


@asynccontextmanager
# cm: def worker_span
async def worker_span(job: Job) -> AsyncIterator[None]:
    print("start", job.id)
    try:
        yield
    finally:
        print("end", job.id)


# cm: def Worker.__init__
class Worker:
    def __init__(self, store: JobStore) -> None:
        self._store = store

    # cm: def Worker.run_once
    async def run_once(self) -> bool:
        job = await self._store.reserve()
        if job is None:
            return False
        # cm: ref Worker.run_once.calls.worker_span
        async with worker_span(job):
            # cm: ref Worker.run_once.calls.asyncio.sleep
            await asyncio.sleep(0)
            await self._store.complete(job.id)
        return True
