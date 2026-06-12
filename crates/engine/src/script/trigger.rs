//! Server script trigger types — mirrors the Engine-TS reference
//! ServerTriggerType.ts. The compiler packs `trigger | kind << 8 |
//! subject << 10` into each script's lookup key.

#![allow(dead_code)]

pub type Trigger = u16;

pub const PROC: Trigger = 0;
pub const LABEL: Trigger = 1;
pub const DEBUGPROC: Trigger = 2;

pub const APNPC1: Trigger = 3;
pub const APNPC2: Trigger = 4;
pub const APNPC3: Trigger = 5;
pub const APNPC4: Trigger = 6;
pub const APNPC5: Trigger = 7;
pub const APNPCU: Trigger = 8;
pub const APNPCT: Trigger = 9;
pub const OPNPC1: Trigger = 10;
pub const OPNPC2: Trigger = 11;
pub const OPNPC3: Trigger = 12;
pub const OPNPC4: Trigger = 13;
pub const OPNPC5: Trigger = 14;
pub const OPNPCU: Trigger = 15;
pub const OPNPCT: Trigger = 16;

pub const APOBJ1: Trigger = 31;
pub const APOBJ2: Trigger = 32;
pub const APOBJ3: Trigger = 33;
pub const APOBJ4: Trigger = 34;
pub const APOBJ5: Trigger = 35;
pub const OPOBJ1: Trigger = 38;
pub const OPOBJ2: Trigger = 39;
pub const OPOBJ3: Trigger = 40;
pub const OPOBJ4: Trigger = 41;
pub const OPOBJ5: Trigger = 42;

pub const APLOC1: Trigger = 59;
pub const APLOC2: Trigger = 60;
pub const APLOC3: Trigger = 61;
pub const APLOC4: Trigger = 62;
pub const APLOC5: Trigger = 63;
pub const OPLOC1: Trigger = 66;
pub const OPLOC2: Trigger = 67;
pub const OPLOC3: Trigger = 68;
pub const OPLOC4: Trigger = 69;
pub const OPLOC5: Trigger = 70;

pub const APPLAYER1: Trigger = 87;
pub const APPLAYER2: Trigger = 88;
pub const APPLAYER3: Trigger = 89;
pub const APPLAYER4: Trigger = 90;
pub const APPLAYER5: Trigger = 91;
pub const OPPLAYER1: Trigger = 94;
pub const OPPLAYER2: Trigger = 95;
pub const OPPLAYER3: Trigger = 96;
pub const OPPLAYER4: Trigger = 97;
pub const OPPLAYER5: Trigger = 98;

pub const QUEUE: Trigger = 116;
/// Npc AI queues 1..20 (Engine-TS `AI_QUEUE1`..): NPC_QUEUE fires
/// `AI_QUEUE1 + queueId - 1`.
pub const AI_QUEUE1: Trigger = 117;
pub const SOFTTIMER: Trigger = 137;
pub const TIMER: Trigger = 138;
pub const AI_TIMER: Trigger = 139;

pub const OPHELD1: Trigger = 140;
pub const OPHELD2: Trigger = 141;
pub const OPHELD3: Trigger = 142;
pub const OPHELD4: Trigger = 143;
pub const OPHELD5: Trigger = 144;
pub const OPHELDU: Trigger = 145;
pub const OPHELDT: Trigger = 146;

pub const IF_BUTTON: Trigger = 147;
pub const IF_CLOSE: Trigger = 148;
pub const INV_BUTTON1: Trigger = 149;
pub const INV_BUTTON2: Trigger = 150;
pub const INV_BUTTON3: Trigger = 151;
pub const INV_BUTTON4: Trigger = 152;
pub const INV_BUTTON5: Trigger = 153;
pub const INV_BUTTOND: Trigger = 154;

pub const WALKTRIGGER: Trigger = 155;
pub const LOGIN: Trigger = 157;
pub const LOGOUT: Trigger = 158;
pub const TUTORIAL: Trigger = 159;
pub const ADVANCESTAT: Trigger = 160;
pub const MAPZONE: Trigger = 161;
pub const MAPZONEEXIT: Trigger = 162;
pub const ZONE: Trigger = 163;
pub const ZONEEXIT: Trigger = 164;
pub const CHANGESTAT: Trigger = 165;
pub const AI_SPAWN: Trigger = 166;
pub const AI_DESPAWN: Trigger = 167;
