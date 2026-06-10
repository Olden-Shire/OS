// @ObfuscatedName("ba") — jag::oldscape::constants::text.
//
// Partial port of Text.java — the most commonly-referenced strings
// that show up in chat / menu / status-bar paths. The full ~150 string
// table is alphabetised in `text.rs` but only the entries we touch
// from cs2 + outbound packet builders are listed here. New entries
// land on demand.
//
// All strings are the English form; rev1 ships German alternates that
// the i18n layer can swap in once it's ported.

#![allow(dead_code)]

// ── Menu verbs ────────────────────────────────────────────────────
pub const TAKE: &str = "Take";
pub const DROP: &str = "Drop";
pub const OK: &str = "Ok";
pub const SELECT: &str = "Select";
pub const CONTINUE: &str = "Continue";
pub const EXAMINE: &str = "Examine";
pub const WALK_HERE: &str = "Walk here";
pub const CLOSE: &str = "Close";
pub const USE: &str = "Use";
pub const CANCEL: &str = "Cancel";
// Text.java:443.
pub const WALKHERE: &str = "Walk here";
pub const CHOOSEOPTION: &str = "Choose Option";
pub const MEMBERS_OBJECT: &str = "Members object";
pub const HIDDEN: &str = "Hidden";

// ── Combat / tooltip labels (Text.java:434/446/449/473) ──────────
pub const ATTACK: &str = "Attack";
pub const LEVEL: &str = "level-";
pub const SKILL: &str = "skill-";
pub const WORLD: &str = "World";

// ── Number short suffixes (Text.java:461/464/467/470) ────────────
pub const MILLION: &str = "M";
pub const MILLION_SHORT: &str = "M";
pub const THOUSAND: &str = "K";
pub const THOUSAND_SHORT: &str = "K";

// ── Status / loading ─────────────────────────────────────────────
pub const LOADING: &str = "Loading - please wait.";
pub const CONLOST: &str = "Connection lost";
pub const ATTEMPT_TO_REESTABLISH: &str = "Please wait - attempting to reestablish";

// ── Mainload progress messages ───────────────────────────────────
pub const MAINLOAD_0: &str = "Starting game engine...";
pub const MAINLOAD_20: &str = "Prepared visibility map";
pub const MAINLOAD_30: &str = "Connecting to update server";
pub const MAINLOAD_40: &str = "Checking for updates - ";
pub const MAINLOAD_40B: &str = "Loaded update list";
pub const MAINLOAD_45: &str = "Prepared sound engine";
pub const MAINLOAD_50: &str = "Loading fonts - ";
pub const MAINLOAD_50B: &str = "Loaded fonts";
pub const MAINLOAD_60: &str = "Loading title screen - ";
pub const MAINLOAD_60B: &str = "Loaded title screen";
pub const MAINLOAD_70: &str = "Loading config - ";
pub const MAINLOAD_70B: &str = "Loaded config";
pub const MAINLOAD_80: &str = "Loading sprites - ";
pub const MAINLOAD_80B: &str = "Loaded sprites";

// ── Chat formatting separators ────────────────────────────────────
pub const MINISEPARATOR: &str = " ";
pub const MORE_OPTIONS: &str = " more options";

// ── Friend system replies ────────────────────────────────────────
pub const TRADEREQ: &str = "wishes to trade with you.";
pub const DUELREQ: &str = "wishes to duel with you.";
pub const FRIENDLISTFULL: &str = "Your friend list is full. Max of 100 for free users, and 200 for members";
pub const FRIENDLISTDUPE: &str = " is already on your friend list";
pub const IGNORELISTFULL: &str = "Your ignore list is full. Max of 100 users.";
pub const IGNORELISTDUPE: &str = " is already on your ignore list";
pub const FRIENDCANTADDSELF: &str = "You can't add yourself to your own friend list";
pub const IGNORECANTADDSELF: &str = "You can't add yourself to your own ignore list";
pub const REMOVEIGNORE1: &str = "Please remove ";
pub const REMOVEIGNORE2: &str = " from your ignore list first";
pub const REMOVEFRIEND1: &str = "Please remove ";
pub const REMOVEFRIEND2: &str = " from your friend list first";
pub const UNABLETOFIND: &str = "Unable to find ";

// ── CS2 chat-prefix markers (English) ────────────────────────────
pub const CHATCOL_YELLOW: &str = "yellow:";
pub const CHATCOL_RED: &str = "red:";
pub const CHATCOL_GREEN: &str = "green:";
pub const CHATCOL_CYAN: &str = "cyan:";
pub const CHATCOL_PURPLE: &str = "purple:";
pub const CHATCOL_WHITE: &str = "white:";
pub const CHATEFFECT_FLASH1: &str = "flash1:";
pub const CHATEFFECT_FLASH2: &str = "flash2:";
pub const CHATEFFECT_FLASH3: &str = "flash3:";
pub const CHATEFFECT_GLOW1: &str = "glow1:";
pub const CHATEFFECT_GLOW2: &str = "glow2:";
pub const CHATEFFECT_GLOW3: &str = "glow3:";
pub const CHATEFFECT_WAVE: &str = "wave:";
pub const CHATEFFECT_WAVE2: &str = "wave2:";
pub const CHATEFFECT_SHAKE: &str = "shake:";
pub const CHATEFFECT_SCROLL: &str = "scroll:";
pub const CHATEFFECT_SLIDE: &str = "slide:";

// ── Additional mainload progress (Text.java:83-101) ───────────────
pub const MAINLOAD_90: &str = "Loading textures - ";
pub const MAINLOAD_90B: &str = "Loaded textures";
pub const MAINLOAD_110: &str = "Loaded input handler";
pub const MAINLOAD_120: &str = "Loading wordpack - ";
pub const MAINLOAD_120B: &str = "Loaded wordpack";
pub const MAINLOAD_130: &str = "Loading interfaces - ";
pub const MAINLOAD_130B: &str = "Loaded interfaces";

// ── World-hop transition (Text.java:104-110) ─────────────────────
pub const LOGINHOP_A: &str = "You have only just left another world.";
pub const LOGINHOP_B: &str = "Your profile will be transferred in:";
pub const LOGINHOP_C: &str = " seconds.";

// ── Login error A/B/C trios (Text.java:113-413) ──────────────────
// Each (A, B, C) triple is the 3 yellow lines shown on the login
// modal for the given server response code.
pub const LOGINM3_A: &str = "Connection timed out.";
pub const LOGINM3_B: &str = "Please try using a different world.";
pub const LOGINM3_C: &str = "";

pub const LOGINM2_A: &str = "";
pub const LOGINM2_B: &str = "Error connecting to server.";
pub const LOGINM2_C: &str = "";

pub const LOGINM1_A: &str = "No response from server.";
pub const LOGINM1_B: &str = "Please try using a different world.";
pub const LOGINM1_C: &str = "";

pub const LOGIN3_A: &str = "";
pub const LOGIN3_B: &str = "Invalid username/email or password.";
pub const LOGIN3_C: &str = "";

pub const LOGIN4_A: &str = "Your account has been disabled.";
pub const LOGIN4_B: &str = "Please check your message-centre for details.";
pub const LOGIN4_C: &str = "";

pub const LOGIN5_A: &str = "Your account is already logged in.";
pub const LOGIN5_B: &str = "Try again in 60 secs...";
pub const LOGIN5_C: &str = "";

pub const LOGIN6_A: &str = "RuneScape has been updated!";
pub const LOGIN6_B: &str = "Please reload this page.";
pub const LOGIN6_C: &str = "";

pub const LOGIN7_A: &str = "This world is full.";
pub const LOGIN7_B: &str = "Please use a different world.";
pub const LOGIN7_C: &str = "";

pub const LOGIN8_A: &str = "Unable to connect.";
pub const LOGIN8_B: &str = "Login server offline.";
pub const LOGIN8_C: &str = "";

pub const LOGIN9_A: &str = "Login limit exceeded.";
pub const LOGIN9_B: &str = "Too many connections from your address.";
pub const LOGIN9_C: &str = "";

pub const LOGIN10_A: &str = "Unable to connect.";
pub const LOGIN10_B: &str = "Bad session id.";
pub const LOGIN10_C: &str = "";

pub const LOGIN11_A: &str = "We suspect someone knows your password.";
pub const LOGIN11_B: &str = "Press 'change your password' on front page.";
pub const LOGIN11_C: &str = "";

pub const LOGIN12_A: &str = "You need a members account to login to this world.";
pub const LOGIN12_B: &str = "Please subscribe, or use a different world.";
pub const LOGIN12_C: &str = "";

pub const LOGIN13_A: &str = "Could not complete login.";
pub const LOGIN13_B: &str = "Please try using a different world.";
pub const LOGIN13_C: &str = "";

pub const LOGIN14_A: &str = "The server is being updated.";
pub const LOGIN14_B: &str = "Please wait 1 minute and try again.";
pub const LOGIN14_C: &str = "";

pub const LOGIN16_A: &str = "Too many incorrect logins from your address.";
pub const LOGIN16_B: &str = "Please wait 5 minutes before trying again.";
pub const LOGIN16_C: &str = "";

pub const LOGIN17_A: &str = "You are standing in a members-only area.";
pub const LOGIN17_B: &str = "To play on this world move to a free area first";
pub const LOGIN17_C: &str = "";

pub const LOGIN18_A: &str = "Account locked as we suspect it has been stolen.";
pub const LOGIN18_B: &str = "Press 'recover a locked account' on front page.";
pub const LOGIN18_C: &str = "";

pub const LOGIN19_A: &str = "This world is running a closed Beta.";
pub const LOGIN19_B: &str = "Sorry invited players only.";
pub const LOGIN19_C: &str = "Please use a different world.";

pub const LOGIN20_A: &str = "Invalid loginserver requested.";
pub const LOGIN20_B: &str = "Please try using a different world.";
pub const LOGIN20_C: &str = "";

pub const LOGIN22_A: &str = "Malformed login packet.";
pub const LOGIN22_B: &str = "Please try again.";
pub const LOGIN22_C: &str = "";

pub const LOGIN23_A: &str = "No reply from loginserver.";
pub const LOGIN23_B: &str = "Please wait 1 minute and try again.";
pub const LOGIN23_C: &str = "";

pub const LOGIN24_A: &str = "Error loading your profile.";
pub const LOGIN24_B: &str = "Please contact customer support.";
pub const LOGIN24_C: &str = "";

pub const LOGIN25_A: &str = "Unexpected loginserver response.";
pub const LOGIN25_B: &str = "Please try using a different world.";
pub const LOGIN25_C: &str = "";

pub const LOGIN26_A: &str = "This computers address has been blocked";
pub const LOGIN26_B: &str = "as it was used to break our rules.";
pub const LOGIN26_C: &str = "";

pub const LOGIN27_A: &str = "";
pub const LOGIN27_B: &str = "Service unavailable.";
pub const LOGIN27_C: &str = "";

pub const LOGIN_USER_LENGTH_A: &str = "";
pub const LOGIN_USER_LENGTH_B: &str = "Please enter your username/email address.";
pub const LOGIN_USER_LENGTH_C: &str = "";

pub const LOGIN_PASS_LENGTH_A: &str = "";
pub const LOGIN_PASS_LENGTH_B: &str = "Please enter your password.";
pub const LOGIN_PASS_LENGTH_C: &str = "";

pub const LOGIN31_A: &str = "Your account must have a displayname set";
pub const LOGIN31_B: &str = "in order to play the game.  Please set it";
pub const LOGIN31_C: &str = "via the website, or the main game.";

pub const LOGIN32_A: &str = "Your attempt to log into your account was";
pub const LOGIN32_B: &str = "unsuccessful.  Don't worry, you can sort";
pub const LOGIN32_C: &str = "this out by visiting the billing system.";

pub const LOGIN37_A: &str = "Your account is currently inaccessible.";
pub const LOGIN37_B: &str = "Please try again in a few minutes.";
pub const LOGIN37_C: &str = "";

pub const LOGIN38_A: &str = "You need to vote to play!";
pub const LOGIN38_B: &str = "Visit runescape.com and vote,";
pub const LOGIN38_C: &str = "and then come back here!";

pub const LOGIN55_A: &str = "Sorry, but your account is not eligible to";
pub const LOGIN55_B: &str = "play this version of the game.  Please try";
pub const LOGIN55_C: &str = "playing the main game instead!";

pub const LOGINMIS_A: &str = "Unexpected server response";
pub const LOGINMIS_B: &str = "Please try using a different world.";
pub const LOGINMIS_C: &str = "";

// ── Misc / modal ─────────────────────────────────────────────────
pub const PLEASEWAIT: &str = "Please wait...";

// ── Title screen / login UI (Text.java:608-662) ───────────────────
pub const PLEASELOGIN2: &str = "Enter your username/email & password.";
pub const CONNECTING2: &str = "Connecting to server...";
pub const USERNAMEPROMPT: &str = "Login: ";
pub const PASSWORDPROMPT: &str = "Password: ";
pub const WELCOMETORUNESCAPE: &str = "Welcome to RuneScape";
pub const NEWUSER: &str = "New User";
pub const EXISTINGUSER: &str = "Existing User";
pub const LOGIN_BUTTON: &str = "Login";
pub const NEWUSER1: &str = "How to Play";
pub const NEWUSER2: &str = "To play Old School RuneScape, you will";
pub const NEWUSER3: &str = "need to be a current RuneScape member,";
pub const NEWUSER4: &str = "and have voted 'Yes' on the poll on the";
pub const NEWUSER5: &str = "RuneScape home page.";

// ── World-select screen (Text.java:665-695) ───────────────────────
pub const SELECTAWORLD: &str = "Select a world";
pub const MEMBERSONLYWORLD: &str = "Members only world";
pub const FREEWORLD: &str = "Free world";
pub const SL_WORLD: &str = "World";
pub const SL_PLAYERS: &str = "Players";
pub const SL_LOCATION: &str = "Location";
pub const SL_TYPE: &str = "Type";
pub const OFFLINEWORLD: &str = "OFF";
pub const FULLWORLD: &str = "FULL";
pub const LOADINGDOTDOTDOT: &str = "Loading...";
pub const CLICKTOSWITCH: &str = "Click to switch";
