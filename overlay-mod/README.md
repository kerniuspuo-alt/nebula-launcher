# Nebula Overlay (Fabric)

Optional **client mod**, installed through Fabric Loader like Sodium. It is not a process injector and does not patch other versions via mixins shipped from the launcher.

- Keybind (default **O**) shows a hint in-game.
- POSTs FPS samples to `http://127.0.0.1:17384/fps` when the launcher is listening.

Wire this into a Fabric 1.21.1 Gradle workspace (`fabric-loom`) when you want a compilable jar. Keep the Minecraft mappings version aligned with your Loom plugin.
