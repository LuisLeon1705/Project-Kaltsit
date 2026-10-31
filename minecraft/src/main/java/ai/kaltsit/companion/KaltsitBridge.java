package ai.kaltsit.companion;

import java.io.*;
import java.net.*;
import java.nio.charset.StandardCharsets;
import java.util.concurrent.*;
import java.util.function.Consumer;

/**
 * High-performance TCP client linking Minecraft tick-by-tick (20 Hz)
 * to Kal'tsit's Rust AI core (127.0.0.1:25599).
 */
public class KaltsitBridge {
    private static final String HOST = "127.0.0.1";
    private static final int PORT = 25599;

    private static volatile boolean running = false;
    private static volatile boolean connected = false;
    private static Socket socket = null;
    private static BufferedWriter writer = null;
    private static BufferedReader reader = null;
    private static final BlockingQueue<String> outQueue = new LinkedBlockingQueue<>();
    private static Consumer<String> onActionReceived = null;
    private static Consumer<String> onCommandReceived = null;
    private static Consumer<String> onChatReceived = null;

    public static void setActionHandler(Consumer<String> handler) {
        onActionReceived = handler;
    }

    public static void setCommandHandler(Consumer<String> handler) {
        onCommandReceived = handler;
    }

    public static void setChatHandler(Consumer<String> handler) {
        onChatReceived = handler;
    }

    public static boolean isConnected() {
        return connected;
    }

    public static void start() {
        if (running) return;
        running = true;
        Thread t = new Thread(KaltsitBridge::clientLoop, "KaltsitBridge-Client");
        t.setDaemon(true);
        t.start();
    }

    public static void send(String jsonLine) {
        if (outQueue.size() < 500) {
            outQueue.offer(jsonLine);
        }
    }

    private static void clientLoop() {
        while (running) {
            try {
                socket = new Socket();
                socket.connect(new InetSocketAddress(HOST, PORT), 1500);
                writer = new BufferedWriter(new OutputStreamWriter(socket.getOutputStream(), StandardCharsets.UTF_8));
                reader = new BufferedReader(new InputStreamReader(socket.getInputStream(), StandardCharsets.UTF_8));
                connected = true;
                send("{\"type\":\"session_start\"}");
                System.out.println("[KaltsitCompanion] Conectado exitosamente con la mente de Kal'tsit (127.0.0.1:25599)!");

                Thread writerThread = new Thread(() -> {
                    try {
                        while (connected && running) {
                            String msg = outQueue.poll(100, TimeUnit.MILLISECONDS);
                            if (msg != null && writer != null) {
                                writer.write(msg);
                                writer.newLine();
                                writer.flush();
                            }
                        }
                    } catch (Exception ignored) {}
                }, "KaltsitBridge-Writer");
                writerThread.setDaemon(true);
                writerThread.start();

                String line;
                while (connected && (line = reader.readLine()) != null) {
                    line = line.trim();
                    if (line.isEmpty()) continue;
                    handleIncoming(line);
                }
            } catch (Exception e) {
                // Connection offline or waiting
            } finally {
                connected = false;
                try { if (socket != null) socket.close(); } catch (Exception ignored) {}
                try { Thread.sleep(2000); } catch (Exception ignored) {}
            }
        }
    }

    private static void handleIncoming(String json) {
        try {
            if (json.contains("\"type\":\"command\"") || json.contains("\"type\": \"command\"")) {
                if (onCommandReceived != null) {
                    onCommandReceived.accept(json);
                }
            } else if (json.contains("\"type\":\"action\"") || json.contains("\"type\": \"action\"")) {
                int idx = json.indexOf("\"action\"");
                if (idx != -1) {
                    int col = json.indexOf(':', idx);
                    int q1 = json.indexOf('"', col);
                    int q2 = json.indexOf('"', q1 + 1);
                    if (q1 != -1 && q2 != -1) {
                        String action = json.substring(q1 + 1, q2);
                        if (onActionReceived != null) onActionReceived.accept(action);
                    }
                }
            } else if (json.contains("\"type\":\"chat\"") || json.contains("\"type\": \"chat\"")) {
                int idx = json.indexOf("\"text\"");
                if (idx != -1) {
                    int col = json.indexOf(':', idx);
                    int q1 = json.indexOf('"', col);
                    int q2 = json.indexOf('"', q1 + 1);
                    if (q1 != -1 && q2 != -1) {
                        String text = json.substring(q1 + 1, q2);
                        if (onChatReceived != null) onChatReceived.accept(text);
                    }
                }
            } else if (json.contains("\"type\":\"hormones\"") || json.contains("\"type\": \"hormones\"") || json.contains("\"tension\"")) {
                float tension = (float) extractDouble(json, "tension", 0.0);
                float rise = (float) extractDouble(json, "rise", 0.5);
                float afecto = (float) extractDouble(json, "afecto", 0.5);
                KaltsitMod.updateHormones(tension, rise, afecto);
            } else if (json.contains("\"type\":\"link_identity\"") || json.contains("\"type\": \"link_identity\"")) {
                int idx = json.indexOf("\"minecraft_username\"");
                if (idx != -1) {
                    int col = json.indexOf(':', idx);
                    int q1 = json.indexOf('"', col);
                    int q2 = json.indexOf('"', q1 + 1);
                    if (q1 != -1 && q2 != -1) {
                        String name = json.substring(q1 + 1, q2);
                        KaltsitMod.setLinkedDoctorName(name);
                    }
                }
            }
        } catch (Exception e) {
            e.printStackTrace();
        }
    }

    private static double extractDouble(String s, String key, double fallback) {
        try {
            int k = s.indexOf("\"" + key + "\"");
            if (k == -1) return fallback;
            int col = s.indexOf(':', k);
            if (col == -1) return fallback;
            int start = col + 1;
            while (start < s.length() && (s.charAt(start) == ' ' || s.charAt(start) == '"')) start++;
            int end = start;
            while (end < s.length() && (Character.isDigit(s.charAt(end)) || s.charAt(end) == '.' || s.charAt(end) == '-')) end++;
            return Double.parseDouble(s.substring(start, end));
        } catch (Exception e) {
            return fallback;
        }
    }
}
