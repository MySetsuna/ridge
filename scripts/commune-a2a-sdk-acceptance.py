"""Official SDK against a supplied existing peer; record fixtures separately."""
import asyncio
import importlib.metadata
import json
from pathlib import Path
import sys
import uuid

import httpx
from a2a.client import ClientConfig, ClientFactory
from a2a.types import a2a_pb2 as p


async def main():
    config_path = Path(sys.argv[1])
    config = json.loads(config_path.read_text(encoding="utf-8"))
    async with httpx.AsyncClient(headers={"Authorization": "Bearer " + config["token"]}, trust_env=False, timeout=30) as http:
        catalog = await http.get(config["baseUrl"] + "/api/v1/a2a/members")
        catalog.raise_for_status()
        cards = catalog.json()["agentCards"]
        assert len(cards) == config.get("expectedMembers", 1), "registered member count mismatch"
        base = cards[0].removesuffix("/.well-known/agent-card.json")
        assert base.startswith(config["baseUrl"] + "/api/v1/a2a/members/")
        client = await ClientFactory(ClientConfig(streaming=False, polling=False, httpx_client=http)).create_from_url(base)

        async def send(text, context_id=""):
            request = p.SendMessageRequest(message=p.Message(message_id=str(uuid.uuid4()), role=p.ROLE_USER, parts=[p.Part(text=text)], context_id=context_id))
            events = [event async for event in client.send_message(request)]
            assert len(events) == 1 and events[0].HasField("task")
            assert events[0].task.status.state != p.TASK_STATE_COMPLETED, "delivery ACK cannot claim objective completion"
            return events[0].task

        async def poll(task_id, state):
            for _ in range(200):
                task = await client.get_task(p.GetTaskRequest(id=task_id))
                assert task.id == task_id and task.context_id == task_id
                if task.status.state == state:
                    return task
                await asyncio.sleep(0.1)
            raise AssertionError("real Agent task did not reach " + p.TaskState.Name(state))

        task = await send(config.get("message", "SIDEBAR_MARKER"))
        ended = await poll(task.id, p.TASK_STATE_INPUT_REQUIRED)
        assert ended.metadata.fields["ridge"].struct_value.fields["execution"].struct_value.fields["state"].string_value == "turn_ended"
        rejected = False
        try:
            await send("MUST_NOT_RUN", context_id="independent-context-not-supported")
        except Exception:
            rejected = True
        assert rejected, "unsupported independent context must fail explicitly"
        pending = await send(config.get("cancelMessage", "CANCEL_WAIT"))
        await poll(pending.id, p.TASK_STATE_WORKING)
        await client.cancel_task(p.CancelTaskRequest(id=pending.id))
        await poll(pending.id, p.TASK_STATE_CANCELED)
        result = {"sdk": importlib.metadata.version("a2a-sdk"), "peerType":config.get("peerType", "unverified-existing-peer"), "pass": True, "taskId": task.id, "cancelTaskId": pending.id,
                  "checks": ["official SDK discovers member card without private target metadata", "same existing Agent receives message and real prompt terminal state", "independent context rejected", "actual protocol cancellation settles as canceled"]}
        config_path.with_name("a2a-sdk-result.json").write_text(json.dumps(result, indent=2), encoding="utf-8")
        print(json.dumps({"pass": True, "sdk": result["sdk"], "checks": result["checks"]}))


asyncio.run(main())
