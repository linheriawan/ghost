# Voice Pipeline State Machine

```
┌──────────────────────────────────────────────────────────────────────┐
│  THREAD: mic-capture (always running in voice mode)                  │
│                                                                      │
│   ●─► Streaming ──[AudioChunk]──► Brain coordinator                 │
└──────────────────────────────────────────────────────────────────────┘
                          │
                       VAD feed
                          │
          ┌───────────────┼───────────────────┐
          │               │                   │
       Silence         SpeechStart         UtteranceEnd
       (update         (update              (send to
        level)          level)              STT thread)
                                               │
┌──────────────────────────────────────────────▼──────────────────────┐
│  THREAD: stt (whisper-base ONNX — separate, non-blocking)           │
│                                                                      │
│   Idle ──[utterance]──► Encoding ──► Decoding ──► [SttResult]──┐   │
│    ▲                                                             │   │
│    └──────────────────── done ───────────────────────────────── ┘   │
│                                                                      │
│   INTERRUPT: new utterance arrives while busy → queue (drop oldest) │
└──────────────────────────────────────────────────────────────────────┘
                                                        │
                                                   SttResult(raw)
                                                        │
┌──────────────────────────────────────────────────────▼──────────────┐
│  THREAD: inference (llama.cpp — one job at a time, FIFO queue)      │
│                                                                      │
│   Idle ──[Correct(raw)]──► Correcting ──[InputDraft stream]──► Idle │
│    │                           │                                     │
│    │                    (short, ~10 tokens,                          │
│    │                     high priority)                              │
│    │                                                                 │
│   Idle ──[Chat(msg)] ──► Inferencing ──[ChatToken stream] ──► Idle  │
│                              │                                       │
│                       (longer, streaming                             │
│                        to chat bubbles)                              │
│                                                                      │
│   INTERRUPT: Chat can be queued while Correcting runs first         │
│   FUTURE:    mistral.rs enables both to batch-run simultaneously    │
└──────────────────────────────────────────────────────────────────────┘
                                    │
                               ChatDone + response text
                                    │
┌───────────────────────────────────▼─────────────────────────────────┐
│  THREAD: tts-playback (persistent sink — NO click between segments) │
│                                                                      │
│   Idle ──[synthesize]──► Kokoro ──[samples]──► Sink.append() ──┐   │
│    ▲                                                             │   │
│    └─────────── sink drains naturally, no open/close ───────── ┘   │
│                                                                      │
│   Mic mute: brain sets mic_mute_until = now + duration              │
└──────────────────────────────────────────────────────────────────────┘

Priority / interrupt rules:
  • STT result always queues Correction BEFORE any pending Chat
  • New utterance during Inferencing → STT runs in parallel, result queues
  • New utterance during Playing → STT runs (mic is muted for echo cancellation)
  • Error in any thread → that thread resets to Idle, status bar shows error

With mistral.rs (future):
  • Correction + Chat can literally run simultaneously (continuous batching)
  • One tokio runtime, two concurrent Request streams
```
