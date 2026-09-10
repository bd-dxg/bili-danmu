import { computed, onMounted, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { DEFAULT_TTS_CONFIG, type TtsConfig, type TtsVoice } from "../types/ipc";

/**
 * 朗读配置的读取 / 编辑 / 落盘，以及音色列表的拉取与有效性校验。
 *
 * 配置改动即时生效（下一段朗读开始用新参数），故每次改动直接落盘，不做「保存」按钮。
 * 统计提示（已保存 / 失败）由页面提供，避免这里再维护一套提示状态。
 */
export function useTtsConfig(
  showSaved: () => void,
  showError: (e: unknown) => void,
) {
  const config = ref<TtsConfig>({ ...DEFAULT_TTS_CONFIG });
  const voices = ref<TtsVoice[]>([]);
  // 试听按钮防连点（合成 + 播放需要一两秒）
  const testing = ref(false);
  // 音色列表拉取中（微软 voices/list 有 300+ 条，首次进入页面后台拉取）
  const loadingVoices = ref(false);

  // 配置里的音色不在列表中（刷新失败时的内置表、旧版手填的残值）：补一项展示，避免下拉框空白
  const customVoice = computed(() =>
    voices.value.length > 0 && !voices.value.some((v) => v.id === config.value.voice)
      ? config.value.voice
      : "",
  );

  async function save() {
    try {
      // 数字输入框清空时 v-model.number 为 ''，避免脏值传给 Rust u32 反序列化报错
      if (!Number.isFinite(config.value.max_len)) {
        config.value.max_len = DEFAULT_TTS_CONFIG.max_len;
      }
      if (!Number.isFinite(config.value.max_queue)) {
        config.value.max_queue = DEFAULT_TTS_CONFIG.max_queue;
      }
      await invoke("tts_set_config", { tts: { ...config.value } });
      showSaved();
    } catch (e) {
      showError(e);
    }
  }

  /** 试听：先落盘再朗读，保证听到的是刚选的音色 / 语速 / 音量 */
  async function testSpeak() {
    testing.value = true;
    try {
      await save();
      await invoke("tts_test_speak", { text: null });
    } catch (e) {
      showError(e);
    } finally {
      setTimeout(() => (testing.value = false), 1500);
    }
  }

  /** 拉取微软完整音色列表；返回是否成功（成功才代表列表权威，可用于校验音色有效性） */
  async function refreshVoices(): Promise<boolean> {
    loadingVoices.value = true;
    try {
      const list = await invoke<TtsVoice[]>("tts_refresh_voices");
      if (list.length) {
        voices.value = list;
        return true;
      }
      return false;
    } catch (e) {
      // 拉取失败保留内置中文音色，音色名仍可手填
      console.error("刷新音色列表失败", e);
      return false;
    } finally {
      loadingVoices.value = false;
    }
  }

  // 配置里的音色不在微软完整列表中时静默回退默认音色：否则每一条弹幕合成都拿不到音频
  // 只在 refreshVoices 成功后调用，见 onMounted 里的说明
  async function fixInvalidVoice() {
    if (!voices.value.length) return;
    if (voices.value.some((v) => v.id === config.value.voice)) return;
    console.warn(`音色 ${config.value.voice} 不在微软音色列表中，已回退默认音色`);
    config.value.voice = DEFAULT_TTS_CONFIG.voice;
    try {
      await invoke("tts_set_config", { tts: { ...config.value } });
    } catch (e) {
      console.error("回退音色失败", e);
    }
  }

  onMounted(async () => {
    try {
      config.value = await invoke<TtsConfig>("tts_get_config");
    } catch (e) {
      console.error("读取朗读配置失败", e);
    }
    // 先上内置列表秒开，再换成微软完整列表
    try {
      voices.value = await invoke<TtsVoice[]>("tts_list_voices");
    } catch (e) {
      console.error("读取音色列表失败", e);
    }
    // 只有拿到微软完整列表才校验：内置的 14 个是兜底子集，拿它校验会把用户
    // 从完整列表里选的音色误判为无效，静默回退并落盘覆盖（离线启动必踩）
    if (await refreshVoices()) {
      await fixInvalidVoice();
    }
  });

  return { config, voices, testing, loadingVoices, customVoice, save, testSpeak, refreshVoices };
}
