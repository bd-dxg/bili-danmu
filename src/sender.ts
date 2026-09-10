// 发送框窗口入口（独立悬浮小窗，背景透明与弹幕窗一致）
import { createApp } from "vue";
import SenderApp from "./sender/SenderApp.vue";
import "./sender.css";

createApp(SenderApp).mount("#app");
