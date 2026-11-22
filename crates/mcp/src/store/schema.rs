//! The store's tables as Diesel sees them. Keep this in step with `migrations/`: the store
//! tests apply every migration and read each table back through these declarations.

diesel::table! {
    notes (name) {
        name -> Text,
        title -> Text,
        body -> Text,
        revision -> BigInt,
        created_at -> Text,
        updated_at -> Text,
    }
}
