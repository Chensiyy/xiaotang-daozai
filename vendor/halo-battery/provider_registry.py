"""上游 v1.18.0 的 HID 提供器注册顺序，协议由上游源码实现。"""

from providers.razer import RazerProvider
from providers.blackshark_v3x import BlackSharkV3XProvider
from providers.audeze import AudezeProvider
from providers.wlmouse import WLmouseProvider
from providers.mchose import MchoseProvider
from providers.hyperx_alpha2 import HyperXAlpha2Provider
from providers.hyperx_cloud3 import HyperXCloud3Provider
from providers.hyperx_cloud_core import HyperXCloudCoreProvider
from providers.hyperx import HyperXProvider
from providers.keychron import KeychronProvider
from providers.pulsar import PulsarProvider
from providers.areson import AresonProvider
from providers.arbiter import ArbiterProvider
from providers.fantech import FantechProvider
from providers.msi import MsiProvider
from providers.wraith import WraithProvider
from providers.machenike import MachenikeProvider
from providers.moondrop import MoondropProvider
from providers.ninjutso_sprime import NinjutsoSprimeProvider
from providers.jbl import JblProvider
from providers.jbl_hidv3 import JblHidV3Provider
from providers.cherry import CherryProvider
from providers.coolermaster import CoolerMasterProvider
from providers.royal_kludge import RoyalKludgeProvider
from providers.zowie import ZowieProvider
from providers.fractal import FractalProvider
from providers.logitech import LogitechProvider
from providers.steelseries import SteelSeriesProvider
from providers.eightbitdo import EightBitDoProvider
from providers.barracuda import BarracudaProvider
from providers.barracuda_x import BarracudaXProvider
from providers.asus import AsusProvider
from providers.gwolves import GWolvesProvider
from providers.aula import AulaProvider
from providers.aula_f108 import AulaF108Provider
from providers.lofree import LofreeProvider
from providers.astro import AstroProvider
from providers.corsair import CorsairProvider
from providers.lamzu import LamzuProvider
from providers.io_aurora import IoAuroraProvider
from providers.vaxee import VaxeeProvider
from providers.am_infinity import AmInfinityProvider
from providers.steelseries_elite import SteelSeriesEliteProvider
from providers.logitech_centurion import LogitechCenturionProvider
from providers.logitech_g435_rtk import LogitechG435RtkProvider
from providers.hyperx_cloud3s import HyperXCloud3SProvider
from providers.nzxt import NzxtProvider
from providers.steam import SteamProvider
from providers.endgame_we import EndgameWEProvider
from providers.mchose_v9 import MchoseV9Provider
from providers.dareu import DareuProvider
from providers.lamzu import LamzuAtlantisProvider
from providers.inphic import InphicProvider
from providers.attackshark_r5 import AttackSharkR5Provider
from providers.turtlebeach import TurtleBeachProvider
from providers.skullcandy import SkullcandyProvider
from providers.rapoo import RapooProvider
from providers.ajazz import AjazzProvider
from providers.blackshark_mtk import BlackSharkMtkProvider
from providers.irocks import IRocksProvider
from providers.sony_inzone import SonyInzoneProvider
from providers.sony_h9ii import SonyH9IIProvider
from providers.atk_nearlink import AtkNearlinkProvider
from providers.finalmouse import FinalmouseProvider
from providers.atk_keyboard import AtkKeyboardProvider
from providers.attackshark_x66 import AttackSharkX66Provider
from providers.attackshark_r11 import AttackSharkR11Provider
from providers.hyperx_alpha_wireless import HyperXAlphaWirelessProvider
from providers.jabra import JabraProvider
from providers.hyperx_cloud_flight import HyperXCloudFlightProvider
from providers.monsgeek import MonsGeekProvider
from providers.royuan import RoyuanProvider
from providers.eweadn import EweadnProvider
from providers.eweadn_ydx import EweadnYdxProvider
from providers.mchose_kb import MchoseKbProvider
from providers.plantronics import PlantronicsProvider
from providers.hyperx_ngenuity import HyperXNgenuityProvider
from providers.alienware import AlienwareProvider
from providers.darmoshark import DarmosharkProvider
from providers.glorious import GloriousProvider
from providers.gs3075t import Gs3075tProvider
from providers.urx import UrxProvider
from providers.mchose_k99 import MchoseK99Provider
from providers.redsquare import RedSquareProvider
from providers.kraken_v4 import KrakenV4Provider
from providers.ninjutso_sora_v3 import NinjutsoSoraV3Provider
from providers.akko_s9 import AkkoS9Provider
from providers.logitech_g325 import LogitechG325Provider
from providers.jbl_quantum350 import JblQuantum350Provider
from providers import beitong, eightbitdo_ultimate2, flydigi, gamesir, raikiri
from providers.base import DeviceStatus, Provider


class GamepadReceivers(Provider):
    """按已限定的接收器复用厂商 HID 查询。"""

    name = "gamepad_receivers"

    def poll(self):
        result = []
        for reader in (beitong, eightbitdo_ultimate2, flydigi, gamesir, raikiri):
            reading = reader.read_connected([])
            if reading is not None:
                result.append(DeviceStatus(
                    reader.__name__, reading.name, reading.level, reading.charging,
                    source=self.name, approx=reading.approx, kind="gamepad",
                    charging_known=getattr(reading, "charging_known", True),
                ))
        return result


PROVIDERS = (
    RazerProvider,
    BlackSharkV3XProvider,
    AudezeProvider,
    WLmouseProvider,
    MchoseProvider,
    HyperXAlpha2Provider,
    HyperXCloud3Provider,
    HyperXCloudCoreProvider,
    HyperXProvider,
    KeychronProvider,
    PulsarProvider,
    AresonProvider,
    ArbiterProvider,
    FantechProvider,
    MsiProvider,
    WraithProvider,
    MachenikeProvider,
    MoondropProvider,
    NinjutsoSprimeProvider,
    JblProvider,
    JblHidV3Provider,
    CherryProvider,
    CoolerMasterProvider,
    RoyalKludgeProvider,
    ZowieProvider,
    FractalProvider,
    LogitechProvider,
    SteelSeriesProvider,
    GamepadReceivers,
    EightBitDoProvider,
    BarracudaProvider,
    BarracudaXProvider,
    AsusProvider,
    GWolvesProvider,
    AulaProvider,
    AulaF108Provider,
    LofreeProvider,
    AstroProvider,
    CorsairProvider,
    LamzuProvider,
    IoAuroraProvider,
    VaxeeProvider,
    AmInfinityProvider,
    SteelSeriesEliteProvider,
    LogitechCenturionProvider,
    LogitechG435RtkProvider,
    HyperXCloud3SProvider,
    NzxtProvider,
    SteamProvider,
    EndgameWEProvider,
    MchoseV9Provider,
    DareuProvider,
    LamzuAtlantisProvider,
    InphicProvider,
    AttackSharkR5Provider,
    TurtleBeachProvider,
    SkullcandyProvider,
    RapooProvider,
    AjazzProvider,
    BlackSharkMtkProvider,
    IRocksProvider,
    SonyInzoneProvider,
    SonyH9IIProvider,
    AtkNearlinkProvider,
    FinalmouseProvider,
    AtkKeyboardProvider,
    AttackSharkX66Provider,
    AttackSharkR11Provider,
    HyperXAlphaWirelessProvider,
    JabraProvider,
    HyperXCloudFlightProvider,
    MonsGeekProvider,
    RoyuanProvider,
    EweadnProvider,
    EweadnYdxProvider,
    MchoseKbProvider,
    PlantronicsProvider,
    HyperXNgenuityProvider,
    AlienwareProvider,
    DarmosharkProvider,
    GloriousProvider,
    Gs3075tProvider,
    UrxProvider,
    MchoseK99Provider,
    RedSquareProvider,
    KrakenV4Provider,
    NinjutsoSoraV3Provider,
    AkkoS9Provider,
    LogitechG325Provider,
    JblQuantum350Provider,
)
