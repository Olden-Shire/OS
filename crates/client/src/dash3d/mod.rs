// jagex3.dash3d — 3D world entities + the software triangle rasterizer
// (Pix3D) + model loaders (ModelUnlit / ModelLit). ClientPlayer + the
// localPlayer tracking live in client_player; the rasterizer and model
// loaders are their own submodules.

pub mod anim_base;
pub mod anim_frame;
pub mod anim_frame_set;
pub mod client_entity;
pub mod client_loc_anim;
pub mod client_npc;
pub mod client_obj;
pub mod client_player;
pub mod client_proj;
pub mod collision_map;
pub mod loc_change;
pub mod map_spot_anim;
pub mod model_source;
pub mod sprite;
pub mod recols_runescape;
pub mod scene_dynamic;
pub mod scene_tile;
pub mod world;
pub mod ground;
pub mod model_lit;
pub mod model_unlit;
pub mod occlude;
pub mod pix3d;
pub mod player_model;
pub mod texture_manager;

pub use client_entity::ClientEntity;
pub use collision_map::CollisionMap;
pub use loc_change::LocChange;
pub use map_spot_anim::MapSpotAnim;
pub use sprite::Sprite as Dash3dSprite;
pub use client_loc_anim::ClientLocAnim;
pub use client_npc::ClientNpc;
pub use client_obj::ClientObj;
pub use client_player::ClientPlayer;
pub use client_proj::ClientProj;
pub use player_model::PlayerModel;
