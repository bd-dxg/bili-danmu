<script setup lang="ts">
import { invoke } from '@tauri-apps/api/core'
import { ref, watch } from 'vue'

import { useLogin } from '../composables/useLogin'

import type { FollowingLive } from '../types/ipc'

// 「关注在播」：已关注且正在开播的主播名，点名字直连。
//
// 登录者自己的直播间由 Rust 排在首位（固定文案「我的直播间」，未开播也显示，
// 便于主播开播前先把软件连上自己房间），前端不重排、只按返回顺序渲染。
//
// 触发时机只有两处：`loggedIn` 变 true 时自动查一次（覆盖「打开软件已是登录态」与
// 「扫码登录成功」两条路径，同一个 watch），之后手动点「刷新」；登出时清空。
//
// 连接动作由父组件做：`useRoomConnection` 是每个页面各自一份的状态与监听，
// 在这里再调一次会多出一套监听与状态，所以只发事件、不自己调用。
defineProps<{
  /** 连接中 / 已连接时锁定（与「最近」面包屑同一套禁用规则） */
  disabled: boolean
}>()

const emit = defineEmits<{ connect: [roomId: number] }>()

const { loggedIn } = useLogin()

const list = ref<FollowingLive[]>([])
const busy = ref(false)
const err = ref('')
/** 是否查过（用来区分「还没查」与「查过但没人在播」） */
const loaded = ref(false)

async function load() {
  busy.value = true
  err.value = ''
  try {
    const next = await invoke<FollowingLive[]>('list_following_live')
    // 查询期间退出登录：丢弃晚到的结果，避免登出后又冒出列表
    if (!loggedIn.value) {
      return
    }
    list.value = next
    loaded.value = true
  } catch (e) {
    if (!loggedIn.value) {
      return
    }
    err.value = String(e)
  } finally {
    busy.value = false
  }
}

watch(loggedIn, on => {
  if (on) {
    void load()
  } else {
    list.value = []
    loaded.value = false
    err.value = ''
  }
})
</script>

<template>
  <div v-if="loggedIn" class="following-bar">
    <span class="label">关注在播</span>
    <button class="refresh" :disabled="busy" @click="load">{{ busy ? '查询中…' : '刷新' }}</button>
    <span v-if="err" class="err">{{ err }}</span>
    <span v-else-if="loaded && !list.length" class="label">（当前没有关注的主播在播）</span>
    <button
      v-for="f in list"
      :key="f.uid"
      class="name"
      :title="`房间 ${f.room_id}`"
      :disabled="disabled"
      @click="emit('connect', f.room_id)">
      {{ f.is_self ? '我的直播间' : f.uname }}
    </button>
  </div>
</template>

<style scoped>
/* 与「最近连接」面包屑同一形态：一行可点直连的名字 */
.following-bar {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 2px 8px;
  margin-top: 10px;
  padding-top: 10px;
  border-top: 1px solid var(--border);
}

.label {
  color: var(--text-faint);
  font-size: 14px;
}

.refresh {
  background: none;
  border: none;
  color: var(--accent);
  font-size: 15px;
  cursor: pointer;
  padding: 2px 6px;
  border-radius: 4px;
}

.refresh:hover:not(:disabled) {
  background: var(--hover);
}

.refresh:disabled {
  color: var(--text-faint);
  cursor: default;
}

.err {
  color: var(--red);
  font-size: 14px;
}

.name {
  background: none;
  border: none;
  padding: 2px 0;
  color: var(--accent);
  font-size: 14px;
  cursor: pointer;
  max-width: 12em;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.name::before {
  content: '›';
  color: var(--text-faint);
  margin-right: 8px;
}

.name:hover:not(:disabled) {
  text-decoration: underline;
}

.name:disabled {
  color: var(--text-faint);
  cursor: not-allowed;
}
</style>
