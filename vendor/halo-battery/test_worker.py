"""可选采集进程的实例归属和读数语义测试。"""
import json
import unittest
from types import SimpleNamespace

from worker import Engine, Gateway, Receiver, ScopedDevice, read_request
from providers.base import DeviceStatus


FIRST = Receiver(0x0B05, 0x1ACE, r"USB\VID_0B05&PID_1ACE\FIRST")
SECOND = Receiver(0x0B05, 0x1ACE, r"USB\VID_0B05&PID_1ACE\SECOND")


class FakeWindows:
    def parent_for(self, path):
        return {"first": FIRST.identity, "second": SECOND.identity}.get(path)

    def paths_by_receiver(self, receivers):
        return {receiver.identity: frozenset([path]) for receiver, path in
                ((FIRST, "first"), (SECOND, "second")) if receiver in receivers}

    def metadata(self, path, receiver):
        return {"path": path, "vendor_id": receiver.vid, "product_id": receiver.pid}


class FakeTransport:
    def device(self):
        return SimpleNamespace(open_path=lambda path: None, read=lambda: [1], close=lambda: None)

    def invoke(self, action, _description):
        return action()


class WorkerTests(unittest.TestCase):
    def test_requires_explicit_parent_identity(self):
        request = {"version": 1, "id": 42, "command": "poll", "receivers": [
            {"vid": FIRST.vid, "pid": FIRST.pid, "usb_instance": FIRST.instance}]}
        self.assertEqual(read_request(json.dumps(request)), (42, [FIRST]))
        request["receivers"][0]["pid"] = 1
        with self.assertRaises(ValueError):
            read_request(json.dumps(request))

    def test_limits_discovery_and_open_to_the_same_receiver(self):
        gate = Gateway(FakeTransport(), FakeWindows())
        gate.update([FIRST, SECOND])
        with gate.scope(gate.inventory[0][FIRST.identity]):
            self.assertEqual([row["path"] for row in gate.enumerate()], ["first"])
            handle = ScopedDevice(gate)
            with self.assertRaises(OSError):
                handle.open_path("second")
            handle.open_path("first")
            self.assertEqual(handle.read(), [1])
            gate.update([SECOND])
            with self.assertRaises(OSError):
                handle.read()

    def test_keeps_charge_only_coarse_and_actual_age(self):
        import time

        gate = Gateway(FakeTransport(), FakeWindows())
        gate.update([FIRST, SECOND])
        engine = Engine.__new__(Engine)
        engine.gate, engine.observed = gate, {}
        now = time.monotonic()
        first_generation = gate.inventory[0][FIRST.identity].generation
        second_generation = gate.inventory[0][SECOND.identity].generation

        charge = DeviceStatus("charge-only", "headset", 80, True, approx="on cable, charging",
                              ui_params={"daozai_instance": FIRST.identity, "daozai_generation": first_generation,
                                         "daozai_observed": now - 30})
        coarse = DeviceStatus("coarse", "mouse", 55, False, approx="about 55% (medium)",
                              ui_params={"daozai_instance": SECOND.identity, "daozai_generation": second_generation,
                                         "daozai_observed": now - 20})

        first = engine.project([charge, coarse], {"charge-only", "coarse"})
        self.assertEqual([device["usb_instance"] for device in first], [FIRST.instance, SECOND.instance])
        self.assertIsNone(first[0]["level"])
        self.assertTrue(first[0]["charging"])
        self.assertTrue(first[0]["fresh"])
        self.assertEqual(first[1]["level"], 55)
        self.assertTrue(first[1]["approximate"])
        again = engine.project([charge, coarse], set())
        self.assertGreaterEqual(again[0]["age_ms"], 30_000)
        self.assertGreaterEqual(again[1]["age_ms"], 20_000)

        gate.update([SECOND])
        gate.update([FIRST, SECOND])
        self.assertEqual([device["key"] for device in engine.project([charge, coarse], set())], ["coarse"])


if __name__ == "__main__":
    unittest.main()
