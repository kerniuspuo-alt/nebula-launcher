package com.nebula.overlay;

import com.mojang.blaze3d.platform.InputConstants;
import net.fabricmc.api.ClientModInitializer;
import net.fabricmc.fabric.api.client.event.lifecycle.v1.ClientTickEvents;
import net.fabricmc.fabric.api.client.keybinding.v1.KeyBindingHelper;
import net.minecraft.client.KeyMapping;
import net.minecraft.client.Minecraft;
import net.minecraft.network.chat.Component;
import org.lwjgl.glfw.GLFW;

import java.io.OutputStream;
import java.net.HttpURLConnection;
import java.net.URI;
import java.util.concurrent.atomic.AtomicBoolean;

/**
 * Companion Fabric mod. Loaded by Fabric like any other client mod.
 * Opens a hint overlay and POSTs FPS to the launcher at 127.0.0.1:17384.
 */
public class NebulaOverlayMod implements ClientModInitializer {
    private KeyMapping open;
    private final AtomicBoolean telemetryInFlight = new AtomicBoolean();
    private int telemetryTicks;

    @Override
    public void onInitializeClient() {
        open = KeyBindingHelper.registerKeyBinding(new KeyMapping(
                "key.nebula.overlay",
                InputConstants.Type.KEYSYM,
                GLFW.GLFW_KEY_O,
                "key.categories.misc"
        ));
        ClientTickEvents.END_CLIENT_TICK.register(client -> {
            while (open.consumeClick()) {
                if (client.player != null) {
                    client.player.displayClientMessage(
                            Component.literal("Nebula overlay — launcher at 127.0.0.1:17384"),
                            true
                    );
                }
            }
            if (client.player != null && ++telemetryTicks >= 20) {
                telemetryTicks = 0;
                postFps(client);
            }
        });
    }

    private void postFps(Minecraft client) {
        if (!telemetryInFlight.compareAndSet(false, true)) return;
        Thread telemetry = new Thread(() -> {
            HttpURLConnection connection = null;
            try {
                byte[] payload = ("{\"fps\":" + client.getFps() + "}")
                        .getBytes(java.nio.charset.StandardCharsets.UTF_8);
                connection = (HttpURLConnection) new URI("http://127.0.0.1:17384/fps").toURL().openConnection();
                connection.setConnectTimeout(500);
                connection.setReadTimeout(500);
                connection.setRequestMethod("POST");
                connection.setDoOutput(true);
                connection.setRequestProperty("Content-Type", "application/json");
                connection.setFixedLengthStreamingMode(payload.length);
                try (OutputStream output = connection.getOutputStream()) {
                    output.write(payload);
                }
                connection.getResponseCode();
            } catch (Exception ignored) {
                // The launcher may not be running.
            } finally {
                if (connection != null) connection.disconnect();
                telemetryInFlight.set(false);
            }
        }, "Nebula FPS telemetry");
        telemetry.setDaemon(true);
        telemetry.start();
    }
}
