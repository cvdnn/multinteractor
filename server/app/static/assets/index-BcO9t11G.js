(function(){const t=document.createElement("link").relList;if(t&&t.supports&&t.supports("modulepreload"))return;for(const o of document.querySelectorAll('link[rel="modulepreload"]'))a(o);new MutationObserver(o=>{for(const r of o)if(r.type==="childList")for(const d of r.addedNodes)d.tagName==="LINK"&&d.rel==="modulepreload"&&a(d)}).observe(document,{childList:!0,subtree:!0});function s(o){const r={};return o.integrity&&(r.integrity=o.integrity),o.referrerPolicy&&(r.referrerPolicy=o.referrerPolicy),o.crossOrigin==="use-credentials"?r.credentials="include":o.crossOrigin==="anonymous"?r.credentials="omit":r.credentials="same-origin",r}function a(o){if(o.ep)return;o.ep=!0;const r=s(o);fetch(o.href,r)}})();class w extends Error{status;constructor(t,s){super(s),this.status=t}}let C=null;function J(e){C=e}async function y(e,t,s=!0){const a=await fetch(e,{credentials:"same-origin",...t,headers:{"Content-Type":"application/json",...t?.headers??{}}});if(!a.ok){let o=`HTTP ${a.status}`;try{const r=await a.json();r.detail&&(o=r.detail)}catch{}throw s&&a.status===401&&C&&C(),new w(a.status,o)}return await a.json()}const b={getSession(){return y("/api/v1/auth/session",{method:"GET"},!1)},login(e){return y("/api/v1/auth/login",{method:"POST",body:JSON.stringify({token:e})},!1)},logout(){return y("/api/v1/auth/logout",{method:"POST"},!1)},getOverview(){return y("/api/v1/overview")},listDevices(e=!1){return y(`/api/v1/devices${e?"?online_only=true":""}`)},kickDevice(e){return y(`/api/v1/devices/${encodeURIComponent(e)}/kick`,{method:"POST"})},disableDevice(e){return y(`/api/v1/devices/${encodeURIComponent(e)}/disable`,{method:"POST"})},enableDevice(e){return y(`/api/v1/devices/${encodeURIComponent(e)}/enable`,{method:"POST"})},removeDevice(e){return y(`/api/v1/devices/${encodeURIComponent(e)}`,{method:"DELETE"})},updateDeviceAcl(e,t,s){return y(`/api/v1/devices/${encodeURIComponent(e)}/acl`,{method:"PATCH",body:JSON.stringify({allowed_categories:t,...s?{subscribed_topics:s}:{}})})},getSecurityTokens(e=!1){return y(`/api/v1/security/tokens?reveal=${e?"true":"false"}`)},issueToken(e,t){return y("/api/v1/devices/token/issue",{method:"POST",body:JSON.stringify({label:e,expected_os:t})})},revokeToken(e){return y(`/api/v1/devices/token/${encodeURIComponent(e)}`,{method:"DELETE"})},rotateClusterToken(e){return y("/api/v1/devices/token/rotate",{method:"POST",body:JSON.stringify(e?{new_token:e}:{})})},listCacheSessions(){return y("/api/v1/cache/sessions")},getCacheSessionDetail(e){return y(`/api/v1/cache/sessions/${encodeURIComponent(e)}`)},pruneCache(e){return y("/api/v1/cache/prune",{method:"POST",body:JSON.stringify({orphan_parts_only:e})})},abortCacheSession(e){return y(`/api/v1/cache/${encodeURIComponent(e)}/abort`,{method:"POST"})},deleteCacheSession(e){return y(`/api/v1/cache/${encodeURIComponent(e)}`,{method:"DELETE"})},queryAuditLogs(e){const t=new URLSearchParams;return e.log_type&&e.log_type!=="ALL"&&t.set("log_type",e.log_type.toLowerCase()),e.device_id&&t.set("device_id",e.device_id),e.status&&t.set("status",e.status),e.keyword&&t.set("keyword",e.keyword),t.set("limit",String(e.limit??50)),t.set("offset",String(e.offset??0)),y(`/api/v1/audit/logs?${t.toString()}`)},getAuditExportUrl(e){const t=new URLSearchParams;return e.log_type&&e.log_type!=="ALL"&&t.set("log_type",e.log_type.toLowerCase()),e.device_id&&t.set("device_id",e.device_id),e.status&&t.set("status",e.status),e.keyword&&t.set("keyword",e.keyword),`/api/v1/audit/export?${t.toString()}`},async exportAuditLogs(e){const t=this.getAuditExportUrl(e),s=await fetch(t,{credentials:"same-origin"});if(!s.ok){let a=`HTTP ${s.status}`;try{const o=await s.json();o.detail&&(a=o.detail)}catch{}throw s.status===401&&C&&C(),new w(s.status,a)}return await s.blob()},cleanupExpiredAudit(){return y("/api/v1/audit/logs/expired",{method:"DELETE"})},getSystemStatus(){return y("/api/v1/system/status")},runWalCheckpoint(){return y("/api/v1/system/wal-checkpoint",{method:"POST"})}};class Y{ws=null;stopped=!1;reconnectDelayMs=1500;timer=null;onMessage;onStateChange;constructor(t,s){this.onMessage=t,this.onStateChange=s}start(){this.stopped=!1,this.connect()}stop(){this.stopped=!0,this.timer!==null&&(window.clearTimeout(this.timer),this.timer=null),this.ws&&(this.ws.close(),this.ws=null),this.onStateChange(!1)}connect(){if(this.stopped)return;const s=`${window.location.protocol==="https:"?"wss:":"ws:"}//${window.location.host}/ws/admin`,a=new WebSocket(s);this.ws=a,a.onopen=()=>{this.reconnectDelayMs=1500,this.onStateChange(!0)},a.onmessage=o=>{try{const r=JSON.parse(String(o.data));this.onMessage(r)}catch{}},a.onclose=o=>{if(this.onStateChange(!1),this.ws=null,o.code===4001||o.code===4003){C&&C();return}this.stopped||(this.timer=window.setTimeout(()=>this.connect(),this.reconnectDelayMs),this.reconnectDelayMs=Math.min(15e3,Math.floor(this.reconnectDelayMs*1.5)))},a.onerror=()=>{a.close()}}}function n(e){return(e==null?"":String(e)).replace(/&/g,"&amp;").replace(/</g,"&lt;").replace(/>/g,"&gt;").replace(/"/g,"&quot;").replace(/'/g,"&#39;")}function f(e){if(!Number.isFinite(e)||e<=0)return"0 B";const t=["B","KB","MB","GB","TB"];let s=0,a=e;for(;a>=1024&&s<t.length-1;)a/=1024,s++;const o=s===0?0:a>=100?1:2;return`${a.toFixed(o)} ${t[s]}`}function O(e){const t=Math.max(0,Math.floor(e));if(t<=0)return"已到期 (待清理)";const s=Math.floor(t/3600),a=Math.floor(t%3600/60),o=t%60;return s>0?`${s}h ${String(a).padStart(2,"0")}m`:a>0?`${a}m ${String(o).padStart(2,"0")}s`:`${o}s`}function X(e){const t=Math.max(0,Math.floor(e)),s=Math.floor(t/86400),a=Math.floor(t%86400/3600),o=Math.floor(t%3600/60),r=t%60;return s>0?`${s}d ${a}h ${o}m`:a>0?`${a}h ${o}m ${r}s`:o>0?`${o}m ${r}s`:`${r}s`}function j(e){if(!e)return"-";const t=new Date(e),s=(a,o=2)=>String(a).padStart(o,"0");return`${t.getFullYear()}-${s(t.getMonth()+1)}-${s(t.getDate())} ${s(t.getHours())}:${s(t.getMinutes())}:${s(t.getSeconds())}`}function Z(e){if(!e)return"-";const t=new Date(e),s=a=>String(a).padStart(2,"0");return`${s(t.getHours())}:${s(t.getMinutes())}:${s(t.getSeconds())}`}function D(e){if(!e)return"-";const t=Math.max(0,Math.floor((Date.now()-e)/1e3));return t<10?"刚刚活跃":t<60?`${t}s 前`:t<3600?`${Math.floor(t/60)} 分钟前`:t<86400?`${Math.floor(t/3600)} 小时前`:`${Math.floor(t/86400)} 天前`}function B(e){return e?e.length<=14?e:`${e.slice(0,6)}...${e.slice(-6)}`:"-"}function ee(e){const t=e.toLowerCase();return t.includes("win")?'<svg class="w-3.5 h-3.5 text-sky-400 shrink-0" fill="currentColor" viewBox="0 0 24 24"><path d="M0 3.449L9.75 2.1v9.451H0m10.949-9.602L24 0v11.4H10.949M0 12.6h9.75v9.451L0 20.699M10.949 12.6H24V24l-12.949-1.801"/></svg>':t.includes("mac")||t.includes("darwin")?'<svg class="w-3.5 h-3.5 text-slate-200 shrink-0" fill="currentColor" viewBox="0 0 24 24"><path d="M18.71 19.5c-.83 1.24-1.71 2.45-3.05 2.47-1.34.03-1.77-.79-3.29-.79-1.53 0-2 .77-3.27.82-1.31.05-2.3-1.32-3.14-2.53C4.25 17 2.94 12.45 4.7 9.39c.87-1.52 2.43-2.48 4.12-2.51 1.28-.02 2.5.87 3.29.87.78 0 2.26-1.07 3.81-.91.65.03 2.47.26 3.64 1.98-.09.06-2.17 1.28-2.15 3.81.03 3.02 2.65 4.03 2.68 4.04-.03.07-.42 1.44-1.38 2.83M15.97 6.37c.61-.75 1.04-1.8 0.92-2.87-.93.04-2.02.63-2.66 1.38-.56.65-.99 1.7-0.88 2.74 1.04.08 2.01-.5 2.62-1.25z"/></svg>':'<svg class="w-3.5 h-3.5 text-amber-400 shrink-0" fill="currentColor" viewBox="0 0 24 24"><path d="M12 2a5 5 0 0 0-5 5v3.5a5 5 0 0 0-3 4.5c0 3 2.5 5 5 5h6c2.5 0 5-2 5-5a5 5 0 0 0-3-4.5V7a5 5 0 0 0-5-5z"/></svg>'}function te(e){const t=e.toLowerCase();return t.includes("win")?"Win":t.includes("mac")||t.includes("darwin")?"Mac":t.includes("linux")||t.includes("ubuntu")?"Lnx":e.slice(0,4)||"OS"}async function R(e){try{if(navigator.clipboard&&window.isSecureContext)return await navigator.clipboard.writeText(e),!0}catch{}const t=document.createElement("textarea");t.value=e,t.style.position="fixed",t.style.left="-9999px",document.body.appendChild(t),t.select();let s=!1;try{s=document.execCommand("copy")}catch{s=!1}return document.body.removeChild(t),s}function U(e,t,s){const a=t.admin_token_is_separate?"控制面已启用独立 MI_ADMIN_TOKEN 认证（数据面 MI_CLUSTER_TOKEN 不可用于登录管理台）。":"服务端未单独配置 MI_ADMIN_TOKEN，当前临时以 MI_CLUSTER_TOKEN 作为管理员登录凭据；建议生产部署时配置 MI_ADMIN_TOKEN。",o=t.admin_token_is_separate?"bg-emerald-500/10 text-emerald-400 border-emerald-500/30":"bg-amber-500/10 text-amber-300 border-amber-500/30";e.innerHTML=`
    <div class="min-h-screen flex items-center justify-center bg-slate-950 px-4 py-12">
      <div class="w-full max-w-md bg-slate-900 border border-slate-800 rounded-2xl p-7 shadow-2xl space-y-6">
        <div class="flex items-center space-x-3">
          <div class="w-10 h-10 rounded-xl bg-gradient-to-tr from-sky-500 to-indigo-600 flex items-center justify-center shadow-lg shadow-sky-500/25">
            <svg class="w-5 h-5 text-white" fill="none" viewBox="0 0 24 24" stroke="currentColor">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 15v2m-6 4h12a2 2 0 002-2v-6a2 2 0 00-2-2H6a2 2 0 00-2 2v6a2 2 0 002 2zm10-10V7a4 4 0 00-8 0v4h8z" />
            </svg>
          </div>
          <div>
            <div class="flex items-center space-x-2">
              <h1 class="text-lg font-bold text-white tracking-wide">multinteractor</h1>
              <span class="text-xs px-2 py-0.5 rounded-full bg-sky-500/10 text-sky-400 font-mono border border-sky-500/20">Cloud Hub</span>
            </div>
            <p class="text-xs text-slate-400 mt-0.5">云端中枢管理控制台 • 强制鉴权门禁</p>
          </div>
        </div>

        <div class="text-xs rounded-xl border p-3 leading-relaxed ${o}">
          ${n(a)}
        </div>

        <form id="auth-login-form" class="space-y-4">
          <div>
            <label for="auth-token-input" class="block text-xs font-medium text-slate-300 mb-1.5">
              ${t.admin_token_is_separate?"管理员密钥 (MI_ADMIN_TOKEN)":"管理员 / 集群访问密钥"}
            </label>
            <div class="relative">
              <input
                id="auth-token-input"
                type="password"
                autocomplete="current-password"
                required
                placeholder="请输入访问凭据..."
                class="w-full bg-slate-950 border border-slate-700 rounded-lg pl-3 pr-14 py-2.5 text-sm font-mono text-white placeholder-slate-500 focus:outline-none focus:border-sky-500"
              />
              <button
                type="button"
                id="auth-toggle-vis"
                class="absolute right-2.5 top-1/2 -translate-y-1/2 text-xs text-slate-400 hover:text-slate-200 px-1.5 py-0.5"
              >显示</button>
            </div>
          </div>

          <div id="auth-error-box" class="hidden rounded-lg bg-rose-500/10 border border-rose-500/30 px-3 py-2 text-xs text-rose-300"></div>

          <button
            type="submit"
            id="auth-submit-btn"
            class="w-full py-2.5 px-4 rounded-lg bg-sky-500 hover:bg-sky-400 text-white text-sm font-semibold transition shadow-lg shadow-sky-500/20"
          >
            登录管理控制台
          </button>
        </form>

        <div class="pt-3 border-t border-slate-800/80 flex items-center justify-between text-[11px] text-slate-500">
          <span>Session Cookie (HttpOnly / SameSite=Strict)</span>
          <span>Constant-Time Guard</span>
        </div>
      </div>
    </div>
  `;const r=document.getElementById("auth-login-form"),d=document.getElementById("auth-token-input"),l=document.getElementById("auth-toggle-vis"),p=document.getElementById("auth-error-box"),i=document.getElementById("auth-submit-btn");l.addEventListener("click",()=>{const c=d.type==="password";d.type=c?"text":"password",l.textContent=c?"隐藏":"显示"}),r.addEventListener("submit",async c=>{c.preventDefault(),p.classList.add("hidden"),i.disabled=!0,i.textContent="验证凭据中...";try{const m=await b.login(d.value);s(m)}catch(m){const g=m instanceof w?m.message:"登录失败，请检查网络连接或凭据。";p.textContent=g,p.classList.remove("hidden")}finally{i.disabled=!1,i.textContent="登录管理控制台"}})}function v(e,t="info"){let s=document.getElementById("toast-container");s||(s=document.createElement("div"),s.id="toast-container",s.className="fixed bottom-5 right-5 z-[60] flex flex-col space-y-2 pointer-events-none max-w-md",document.body.appendChild(s));const a=t==="error"?"bg-rose-400":t==="warn"?"bg-amber-400":t==="success"?"bg-emerald-400":"bg-sky-400",o=t==="error"?"border-rose-500/40":t==="warn"?"border-amber-500/40":"border-slate-700",r=document.createElement("div");r.className=`px-4 py-2.5 rounded-lg bg-slate-950/95 border ${o} text-slate-100 text-xs shadow-2xl flex items-center space-x-2.5 transition-all duration-300 transform translate-y-2 opacity-0 pointer-events-auto`;const d=document.createElement("span");d.className=`w-2 h-2 rounded-full shrink-0 ${a}`;const l=document.createElement("span");l.textContent=e,r.appendChild(d),r.appendChild(l),s.appendChild(r),window.setTimeout(()=>{r.classList.remove("translate-y-2","opacity-0")},20),window.setTimeout(()=>{r.classList.add("opacity-0","translate-y-2"),window.setTimeout(()=>r.remove(),300)},3600)}function K(e){document.getElementById("mi-modal-overlay")?.remove();const s=document.createElement("div");s.id="mi-modal-overlay",s.className="fixed inset-0 bg-slate-950/80 backdrop-blur-sm z-50 flex items-center justify-center p-4",s.innerHTML=`
    <div class="bg-slate-900 border border-slate-800 rounded-xl max-w-md w-full p-5 shadow-2xl space-y-4">
      <div class="flex items-center justify-between border-b border-slate-800 pb-3">
        <h3 class="text-sm font-bold text-white flex items-center space-x-2">
          <svg class="w-4 h-4 text-amber-400" fill="none" viewBox="0 0 24 24" stroke="currentColor">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 9v2m0 4h.01m-6.938 4h13.856c1.54 0 2.502-1.667 1.732-3L13.732 4c-.77-1.333-2.694-1.333-3.464 0L3.34 16c-.77 1.333.192 3 1.732 3z" />
          </svg>
          <span>轮转主集群预共享密钥 (MI_CLUSTER_TOKEN)</span>
        </h3>
        <button id="modal-close-x" class="text-slate-400 hover:text-slate-200 text-sm px-1">✕</button>
      </div>

      <div class="text-xs text-slate-300 leading-relaxed space-y-2">
        <p><strong class="text-amber-300">安全影响：</strong>轮转将会立即吊销全部旧数据面 Token（含历史签发的设备专属 Token）。所有客户端 <code class="font-mono text-sky-300">mi-daemon</code> 重连时需更新新 Token。</p>
        <p class="text-slate-400">当前 WebUI 管理员会话受独立 Session 保护，轮转不会使本管理后台掉线。</p>
      </div>

      <div class="space-y-1.5">
        <label class="text-xs text-slate-400 block">新集群密钥（留空将由服务端密码学随机生成）:</label>
        <input type="text" id="rotate-new-token-input" placeholder="留空自动生成高强随机密钥" class="w-full bg-slate-950 border border-slate-800 rounded-lg px-3 py-2 text-xs font-mono text-white placeholder-slate-600 focus:outline-none focus:border-amber-500" />
      </div>

      <div id="rotate-result-box" class="hidden rounded-lg bg-emerald-500/10 border border-emerald-500/30 p-3 space-y-2 text-xs">
        <div class="text-emerald-300 font-semibold">集群密钥轮转成功，新密钥已生效：</div>
        <div class="flex items-center space-x-2 bg-slate-950 rounded px-2.5 py-1.5 border border-slate-800">
          <code id="rotate-result-token" class="font-mono text-sky-300 break-all flex-1"></code>
          <button id="rotate-copy-btn" class="text-sky-400 hover:text-sky-300 shrink-0 font-medium">复制</button>
        </div>
      </div>

      <div class="flex items-center justify-end space-x-2 pt-2 border-t border-slate-800">
        <button id="modal-cancel-btn" class="px-3 py-1.5 rounded-lg text-xs text-slate-400 hover:text-slate-200">关闭</button>
        <button id="rotate-submit-btn" class="px-3 py-1.5 rounded-lg text-xs font-semibold bg-amber-500 hover:bg-amber-400 text-slate-950">确认轮转并生效</button>
      </div>
    </div>
  `,document.body.appendChild(s);const a=()=>s.remove();s.querySelector("#modal-close-x")?.addEventListener("click",a),s.querySelector("#modal-cancel-btn")?.addEventListener("click",a);const o=s.querySelector("#rotate-submit-btn"),r=s.querySelector("#rotate-new-token-input"),d=s.querySelector("#rotate-result-box"),l=s.querySelector("#rotate-result-token");s.querySelector("#rotate-copy-btn").addEventListener("click",async()=>{const i=await R(l.textContent||"");v(i?"新集群 Token 已复制到剪贴板":"复制失败，请手动选中复制",i?"success":"warn")}),o.addEventListener("click",async()=>{o.disabled=!0,o.textContent="正在轮转...";try{const i=await b.rotateClusterToken(r.value.trim()||void 0);l.textContent=i.token,d.classList.remove("hidden"),o.classList.add("hidden"),v("主集群密钥轮转成功！所有旧客户端 Token 已吊销。","warn"),e()}catch(i){v(i instanceof w?i.message:"密钥轮转失败","error"),o.disabled=!1,o.textContent="确认轮转并生效"}})}function W(e){document.getElementById("mi-modal-overlay")?.remove();const s=document.createElement("div");s.id="mi-modal-overlay",s.className="fixed inset-0 bg-slate-950/80 backdrop-blur-sm z-50 flex items-center justify-center p-4",s.innerHTML=`
    <div class="bg-slate-900 border border-slate-800 rounded-xl max-w-md w-full p-5 shadow-2xl space-y-4">
      <div class="flex items-center justify-between border-b border-slate-800 pb-3">
        <h3 class="text-sm font-bold text-white">签发设备专属 Access Token</h3>
        <button id="modal-close-x" class="text-slate-400 hover:text-slate-200 text-sm px-1">✕</button>
      </div>

      <div class="space-y-3 text-xs">
        <div>
          <label class="text-slate-400 block mb-1">设备别名 / 用途说明:</label>
          <input type="text" id="issue-label-input" placeholder="例如：Handy-MacStudio" class="w-full bg-slate-950 border border-slate-800 rounded-lg px-3 py-2 text-white focus:outline-none focus:border-sky-500" />
        </div>
        <div>
          <label class="text-slate-400 block mb-1">预期操作系统:</label>
          <select id="issue-os-select" class="w-full bg-slate-950 border border-slate-800 rounded-lg px-3 py-2 text-white focus:outline-none">
            <option value="windows">Windows</option>
            <option value="macos">macOS</option>
            <option value="linux">Linux</option>
          </select>
        </div>
      </div>

      <div id="issued-secret-box" class="hidden rounded-lg bg-emerald-500/10 border border-emerald-500/30 p-3 space-y-2 text-xs">
        <div class="text-emerald-300 font-semibold">专属 Token 已签发（明文仅展示这一次，请立即复制保存）：</div>
        <div class="flex items-center space-x-2 bg-slate-950 rounded px-2.5 py-2 border border-slate-800">
          <code id="issued-secret-value" class="font-mono text-sky-300 break-all flex-1"></code>
          <button id="issued-copy-btn" class="text-sky-400 hover:text-sky-300 shrink-0 font-medium">复制</button>
        </div>
      </div>

      <div class="flex items-center justify-end space-x-2 pt-2 border-t border-slate-800">
        <button id="modal-cancel-btn" class="px-3 py-1.5 rounded-lg text-xs text-slate-400 hover:text-slate-200">关闭</button>
        <button id="issue-submit-btn" class="px-3 py-1.5 rounded-lg text-xs font-semibold bg-sky-500 hover:bg-sky-400 text-white">立即签发</button>
      </div>
    </div>
  `,document.body.appendChild(s);const a=()=>s.remove();s.querySelector("#modal-close-x")?.addEventListener("click",a),s.querySelector("#modal-cancel-btn")?.addEventListener("click",a);const o=s.querySelector("#issue-label-input"),r=s.querySelector("#issue-os-select"),d=s.querySelector("#issue-submit-btn"),l=s.querySelector("#issued-secret-box"),p=s.querySelector("#issued-secret-value");s.querySelector("#issued-copy-btn").addEventListener("click",async()=>{const c=await R(p.textContent||"");v(c?"设备 Token 已复制":"复制失败",c?"success":"warn")}),d.addEventListener("click",async()=>{d.disabled=!0,d.textContent="签发中...";try{const c=await b.issueToken(o.value.trim(),r.value);p.textContent=c.token,l.classList.remove("hidden"),d.classList.add("hidden"),v(`已签发专属 Token (${c.token_prefix})`,"success"),e(c)}catch(c){v(c instanceof w?c.message:"签发失败","error"),d.disabled=!1,d.textContent="立即签发"}})}function G(e,t){document.getElementById("mi-drawer-overlay")?.remove();const a=new Set(e.allowed_categories),o=document.createElement("div");o.id="mi-drawer-overlay",o.className="fixed inset-0 bg-slate-950/60 backdrop-blur-[1px] z-50 flex justify-end",o.innerHTML=`
    <div class="w-96 max-w-full bg-slate-950 border-l border-slate-800 shadow-2xl h-full p-6 flex flex-col justify-between">
      <div class="overflow-y-auto pr-1">
        <div class="flex items-center justify-between border-b border-slate-800 pb-3">
          <h3 class="font-bold text-white text-sm">设备配置与 ACL: ${n(e.device_name)}</h3>
          <button id="drawer-close-x" class="text-slate-400 hover:text-slate-200 px-1">✕</button>
        </div>

        <div class="mt-4 space-y-4 text-xs">
          <div class="grid grid-cols-2 gap-3">
            <div>
              <span class="text-slate-500">设备 ID:</span>
              <div class="font-mono text-slate-200 font-semibold mt-0.5 break-all">${n(e.device_id)}</div>
            </div>
            <div>
              <span class="text-slate-500">操作系统:</span>
              <div class="font-mono text-slate-300 mt-0.5">${n(e.os)}</div>
            </div>
          </div>

          <div>
            <span class="text-slate-500">内网即用即连端点 (Client Reported):</span>
            <div class="font-mono text-sky-400 mt-0.5">${n(e.lan_endpoint||"未上报")}</div>
          </div>

          <div class="pt-3 border-t border-slate-800/80">
            <span class="text-slate-300 font-semibold block mb-2">已订阅 Topic 权限列表 (防跨机越权):</span>
            <div class="space-y-1.5 font-mono text-[11px] text-slate-400 bg-slate-900/70 rounded-lg p-2.5 border border-slate-800">
              ${e.subscribed_topics.map(l=>`<div class="flex items-center space-x-2">
                    <span class="w-1.5 h-1.5 rounded-full bg-sky-400 shrink-0"></span>
                    <span class="break-all">${n(l)}</span>
                  </div>`).join("")}
            </div>
          </div>

          <div class="pt-3 border-t border-slate-800/80">
            <span class="text-slate-300 font-semibold block mb-1">内容分类白名单过滤 (allowed_categories):</span>
            <p class="text-[11px] text-slate-500 mb-2.5">取消勾选将在 Cloud Hub 路由投递阶段拦截对应类型事件，防止敏感文件进入受限设备。</p>
            <div class="space-y-2.5 text-slate-200">
              <label class="flex items-center space-x-2.5 cursor-pointer">
                <input type="checkbox" data-cat="TEXT" ${a.has("TEXT")?"checked":""} class="acl-cat-checkbox rounded bg-slate-900 border-slate-700 text-sky-500" />
                <span>TEXT (剪贴板文本)</span>
              </label>
              <label class="flex items-center space-x-2.5 cursor-pointer">
                <input type="checkbox" data-cat="IMAGE" ${a.has("IMAGE")?"checked":""} class="acl-cat-checkbox rounded bg-slate-900 border-slate-700 text-sky-500" />
                <span>IMAGE (剪贴板图片)</span>
              </label>
              <label class="flex items-center space-x-2.5 cursor-pointer">
                <input type="checkbox" data-cat="FILE" ${a.has("FILE")?"checked":""} class="acl-cat-checkbox rounded bg-slate-900 border-slate-700 text-sky-500" />
                <span>FILE (单个大文件传输)</span>
              </label>
              <label class="flex items-center space-x-2.5 cursor-pointer">
                <input type="checkbox" data-cat="DIRECTORY" ${a.has("DIRECTORY")?"checked":""} class="acl-cat-checkbox rounded bg-slate-900 border-slate-700 text-sky-500" />
                <span>DIRECTORY (文件夹 .tar.zst)</span>
              </label>
            </div>
          </div>
        </div>
      </div>

      <div class="pt-4 border-t border-slate-800 flex items-center justify-end space-x-2">
        <button id="drawer-cancel-btn" class="px-3 py-1.5 rounded-lg text-xs text-slate-400 hover:text-slate-200">关闭</button>
        <button id="drawer-save-btn" class="px-3.5 py-1.5 rounded-lg text-xs font-semibold bg-sky-500 hover:bg-sky-400 text-white">保存 ACL 策略</button>
      </div>
    </div>
  `,document.body.appendChild(o);const r=()=>o.remove();o.addEventListener("click",l=>{l.target===o&&r()}),o.querySelector("#drawer-close-x")?.addEventListener("click",r),o.querySelector("#drawer-cancel-btn")?.addEventListener("click",r);const d=o.querySelector("#drawer-save-btn");d.addEventListener("click",async()=>{const l=[];o.querySelectorAll(".acl-cat-checkbox").forEach(p=>{p.checked&&p.dataset.cat&&l.push(p.dataset.cat)}),d.disabled=!0,d.textContent="保存中...";try{const p=await b.updateDeviceAcl(e.device_id,l);v(`设备 ${e.device_name} 的 ACL 白名单已更新`,"success"),r(),t(p.device)}catch(p){v(p instanceof w?p.message:"ACL 保存失败","error"),d.disabled=!1,d.textContent="保存 ACL 策略"}})}async function se(e){document.getElementById("mi-modal-overlay")?.remove();let s;try{s=await b.getCacheSessionDetail(e)}catch(r){v(r instanceof w?r.message:"获取缓存分块详情失败","error");return}const a=document.createElement("div");a.id="mi-modal-overlay",a.className="fixed inset-0 bg-slate-950/80 backdrop-blur-sm z-50 flex items-center justify-center p-4",a.innerHTML=`
    <div class="bg-slate-900 border border-slate-800 rounded-xl max-w-2xl w-full p-5 shadow-2xl space-y-4 max-h-[85vh] flex flex-col">
      <div class="flex items-center justify-between border-b border-slate-800 pb-3 shrink-0">
        <div>
          <h3 class="text-sm font-bold text-white font-mono">缓存分块清单与摘要追溯: ${n(s.file_cache_id)}</h3>
          <p class="text-xs text-slate-400 mt-0.5">${n(s.file_name)} • ${f(s.total_size)} • 24h TTL 剩余 ${O(s.ttl_remaining_seconds)}</p>
        </div>
        <button id="modal-close-x" class="text-slate-400 hover:text-slate-200 text-sm px-1">✕</button>
      </div>

      <div class="space-y-3 text-xs overflow-y-auto pr-1">
        <div class="bg-slate-950 border border-slate-800 rounded-lg p-3 space-y-1.5 font-mono">
          <div class="text-slate-400">全文 BLAKE3 摘要 (${n(s.digest.algorithm)}):</div>
          <div class="text-emerald-400 break-all">${n(s.digest.full_hash)}</div>
          <div class="text-slate-500 pt-1">分块尺寸: ${f(s.digest.chunk_size_bytes)} • 分块数: ${s.digest.chunk_hashes.length}</div>
        </div>

        <div class="space-y-1.5">
          <div class="text-slate-400 font-semibold">4MB 固定分块 BLAKE3 清单 (${s.verified_chunks} / ${s.total_chunks} 已验):</div>
          <div class="bg-slate-950 border border-slate-800 rounded-lg divide-y divide-slate-800/70 max-h-60 overflow-y-auto font-mono text-[11px]">
            ${s.digest.chunk_hashes.length===0?'<div class="p-3 text-slate-500">空文件 (0 分块)</div>':s.digest.chunk_hashes.map((r,d)=>`<div class="px-3 py-1.5 flex items-center justify-between gap-3">
                        <span class="text-slate-400 shrink-0">#${String(d).padStart(4,"0")}</span>
                        <span class="text-slate-200 break-all">${n(r)}</span>
                        <span class="shrink-0 ${d<s.verified_chunks?"text-emerald-400":"text-slate-500"}">
                          ${d<s.verified_chunks?"VERIFIED":"PENDING"}
                        </span>
                      </div>`).join("")}
          </div>
        </div>
      </div>

      <div class="flex justify-end pt-2 border-t border-slate-800 shrink-0">
        <button id="modal-cancel-btn" class="px-3 py-1.5 rounded-lg text-xs bg-slate-800 hover:bg-slate-700 text-slate-200">关闭</button>
      </div>
    </div>
  `,document.body.appendChild(a);const o=()=>a.remove();a.addEventListener("click",r=>{r.target===a&&o()}),a.querySelector("#modal-close-x")?.addEventListener("click",o),a.querySelector("#modal-cancel-btn")?.addEventListener("click",o)}function ae(e){const t=j(e.timestamp_ms);if(e.record_type==="TRANSFER"){const a=e.verification_status==="VERIFIED_OK";return`
      <tr class="hover:bg-slate-900/50">
        <td class="py-3 px-4 text-slate-400">
          <div class="text-slate-200">#${e.id}</div>
          <div class="text-[10px] text-slate-500">${n(t)}</div>
        </td>
        <td class="py-3 px-4">
          <span class="px-2 py-0.5 rounded bg-indigo-500/10 text-indigo-400 border border-indigo-500/20 text-[10px]">audit_transfer_log</span>
        </td>
        <td class="py-3 px-4 font-sans">
          <span class="font-medium text-white">${n(e.source_device_id)}</span> → <span class="font-medium text-slate-300">${n(e.target_device_id)}</span>
        </td>
        <td class="py-3 px-4">
          <div class="text-sky-300">${n(e.channel_used)}</div>
          <div class="text-[10px] ${e.resumed?"text-amber-400":"text-slate-500"}">
            ${e.resumed?`已续传 ${e.resume_count??1} 次 • 偏移 ${e.resume_offset??0}`:`cache: ${n(e.file_cache_id||"-")}`}
          </div>
        </td>
        <td class="py-3 px-4">
          <div class="text-slate-200">${f(e.bytes_transferred??0)}</div>
          <div class="text-[10px] text-slate-400">b3: ${n(B(e.content_hash))}</div>
        </td>
        <td class="py-3 px-4">
          <span class="px-2 py-0.5 rounded ${a?"bg-emerald-500/10 text-emerald-400":"bg-rose-500/10 text-rose-400"} text-[11px] font-sans">${n(e.verification_status)}</span>
        </td>
      </tr>
    `}if(e.record_type==="EVENT"){const a=e.target_device_ids??[];return`
      <tr class="hover:bg-slate-900/50">
        <td class="py-3 px-4 text-slate-400">
          <div class="text-slate-200">#${e.id}</div>
          <div class="text-[10px] text-slate-500">${n(t)}</div>
        </td>
        <td class="py-3 px-4">
          <span class="px-2 py-0.5 rounded bg-sky-500/10 text-sky-400 border border-sky-500/20 text-[10px]">audit_event_log</span>
        </td>
        <td class="py-3 px-4 font-sans">
          <span class="font-medium text-white">${n(e.source_device_id)}</span> → <span class="text-slate-400">${n(a.join(", ")||"0 台")}</span>
        </td>
        <td class="py-3 px-4">
          <div class="text-slate-300">${n(e.topic)}</div>
          <div class="text-[10px] text-slate-500">${n(e.event_id)} (${n(e.content_category)})</div>
        </td>
        <td class="py-3 px-4">
          <div class="text-slate-200">${f(e.size_bytes??0)}</div>
          <div class="text-[10px] text-slate-400">b3: ${n(B(e.content_hash))}</div>
        </td>
        <td class="py-3 px-4">
          <span class="px-2 py-0.5 rounded bg-emerald-500/10 text-emerald-400 text-[11px] font-sans">DELIVERED</span>
        </td>
      </tr>
    `}const s=e.status==="OK";return`
    <tr class="hover:bg-slate-900/50">
      <td class="py-3 px-4 text-slate-400">
        <div class="text-slate-200">#${e.id}</div>
        <div class="text-[10px] text-slate-500">${n(t)}</div>
      </td>
      <td class="py-3 px-4">
        <span class="px-2 py-0.5 rounded bg-emerald-500/10 text-emerald-400 border border-emerald-500/20 text-[10px]">audit_connection_log</span>
      </td>
      <td class="py-3 px-4 font-sans">
        <span class="font-medium text-white">${n(e.device_name||e.device_id)}</span>
        <div class="text-[10px] text-slate-500 font-mono">${n(e.device_id)}</div>
      </td>
      <td class="py-3 px-4">
        <div class="text-slate-200">${n(e.action)}</div>
        <div class="text-[10px] text-slate-400">${n(e.lan_endpoint||"-")}</div>
      </td>
      <td class="py-3 px-4 text-slate-400 max-w-xs truncate">${n(e.detail||"-")}</td>
      <td class="py-3 px-4">
        <span class="px-2 py-0.5 rounded ${s?"bg-emerald-500/10 text-emerald-400":"bg-rose-500/10 text-rose-400"} text-[11px] font-sans">${n(e.status)}</span>
      </td>
    </tr>
  `}function ne(e,t,s,a,o){const d=[{id:"ALL",label:`全部审计日志 (${t.total_counts.connection+t.total_counts.event+t.total_counts.transfer})`},{id:"TRANSFER",label:`传输与校验 (${t.total_counts.transfer})`},{id:"EVENT",label:`事件路由 (${t.total_counts.event})`},{id:"CONNECTION",label:`连接准入 (${t.total_counts.connection})`}].map(u=>{const x=a.logType===u.id?"bg-sky-500/20 text-sky-400 border-sky-500/30":"text-slate-400 hover:text-slate-200 hover:bg-slate-900 border-transparent";return`<button data-log-type="${u.id}" class="audit-tab-btn px-3 py-1.5 rounded-lg text-xs font-medium border transition ${x}">${n(u.label)}</button>`}).join(""),l=['<option value="">全部设备</option>',...s.map(u=>`<option value="${n(u.device_id)}" ${a.deviceId===u.device_id?"selected":""}>${n(u.device_name)} (${n(u.device_id)})</option>`)].join(""),p=t.total===0?0:a.offset+1,i=Math.min(t.total,a.offset+t.items.length);e.innerHTML=`
    <div class="space-y-6">
      <div class="flex flex-col sm:flex-row sm:items-center justify-between gap-4">
        <div>
          <h2 class="text-lg font-bold text-white">全链路日志审计与取证 (SQLite WAL Audit)</h2>
          <p class="text-xs text-slate-400">记录连接鉴权、事件广播路由、文件流式传输与 BLAKE3 校验回执，支持多维过滤与 JSON 导出</p>
        </div>
        <div class="flex items-center space-x-2">
          <button
            id="audit-export-btn"
            type="button"
            class="px-3 py-1.5 rounded-lg text-xs font-medium bg-slate-800 hover:bg-slate-700 text-slate-200 transition border border-slate-700"
          >导出审计 JSON</button>
        </div>
      </div>

      <div class="flex flex-wrap items-center gap-2 border-b border-slate-800 pb-3">${d}</div>

      <div class="flex flex-wrap items-center gap-3 bg-slate-950/60 border border-slate-800 p-3 rounded-xl text-xs">
        <div class="flex items-center space-x-2">
          <span class="text-slate-400">设备:</span>
          <select id="audit-device-select" class="bg-slate-900 border border-slate-800 rounded px-2.5 py-1.5 text-slate-200 focus:outline-none">
            ${l}
          </select>
        </div>

        <div class="flex items-center space-x-2">
          <span class="text-slate-400">校验/状态:</span>
          <select id="audit-status-select" class="bg-slate-900 border border-slate-800 rounded px-2.5 py-1.5 text-slate-200 focus:outline-none">
            <option value="" ${a.status===""?"selected":""}>全部状态</option>
            <option value="VERIFIED_OK" ${a.status==="VERIFIED_OK"?"selected":""}>正常 / VERIFIED_OK</option>
            <option value="RESUMED" ${a.status==="RESUMED"?"selected":""}>断点续传 (Resumed)</option>
            <option value="FAILED" ${a.status==="FAILED"?"selected":""}>异常 / 鉴权或校验失败</option>
          </select>
        </div>

        <div class="flex items-center space-x-2 flex-1 min-w-[200px] max-w-sm">
          <input
            type="text"
            id="audit-keyword-input"
            value="${n(a.keyword)}"
            placeholder="搜索 event_id / file_cache_id / hash / IP..."
            class="w-full bg-slate-900 border border-slate-800 rounded px-2.5 py-1.5 text-slate-200 placeholder-slate-500 focus:outline-none focus:border-sky-500"
          />
          <button id="audit-search-btn" class="px-3 py-1.5 rounded bg-sky-500 hover:bg-sky-400 text-white font-medium shrink-0">检索</button>
        </div>

        <div class="ml-auto flex items-center space-x-3 text-slate-500 font-mono">
          <span>SQLite WAL 查询: ${t.query_ms}ms</span>
        </div>
      </div>

      <div class="bg-slate-950/70 border border-slate-800 rounded-xl overflow-x-auto shadow-sm">
        <table class="w-full text-left text-xs text-slate-300">
          <thead class="bg-slate-900/90 text-slate-400 border-b border-slate-800 uppercase font-mono text-[11px]">
            <tr>
              <th class="py-3 px-4">序号 / 时间戳</th>
              <th class="py-3 px-4">审计表类型</th>
              <th class="py-3 px-4">源设备 / 目标</th>
              <th class="py-3 px-4">通道 / 行为 / 主题</th>
              <th class="py-3 px-4">BLAKE3 摘要 / 详情</th>
              <th class="py-3 px-4">状态回执</th>
            </tr>
          </thead>
          <tbody class="divide-y divide-slate-800/80 font-mono">
            ${t.items.length===0?'<tr><td colspan="6" class="py-10 text-center text-slate-500 font-sans">当前筛选条件下无匹配审计记录</td></tr>':t.items.map(ae).join("")}
          </tbody>
        </table>
      </div>

      <div class="flex items-center justify-between text-xs text-slate-400">
        <span class="font-mono">显示 ${p} - ${i} / 共 ${t.total} 条</span>
        <div class="flex items-center space-x-2">
          <button id="audit-prev-btn" ${a.offset<=0?"disabled":""} class="px-3 py-1.5 rounded-lg bg-slate-900 border border-slate-800 hover:bg-slate-800 disabled:opacity-40">上一页</button>
          <button id="audit-next-btn" ${i>=t.total?"disabled":""} class="px-3 py-1.5 rounded-lg bg-slate-900 border border-slate-800 hover:bg-slate-800 disabled:opacity-40">下一页</button>
        </div>
      </div>
    </div>
  `,e.querySelectorAll(".audit-tab-btn").forEach(u=>{u.addEventListener("click",()=>{a.logType=u.dataset.logType??"ALL",a.offset=0,o()})});const c=e.querySelector("#audit-device-select");c.addEventListener("change",()=>{a.deviceId=c.value,a.offset=0,o()});const m=e.querySelector("#audit-status-select");m.addEventListener("change",()=>{a.status=m.value,a.offset=0,o()});const g=e.querySelector("#audit-keyword-input"),$=()=>{a.keyword=g.value.trim(),a.offset=0,o()};e.querySelector("#audit-search-btn")?.addEventListener("click",$),g.addEventListener("keydown",u=>{u.key==="Enter"&&$()}),e.querySelector("#audit-prev-btn")?.addEventListener("click",()=>{a.offset=Math.max(0,a.offset-a.limit),o()}),e.querySelector("#audit-next-btn")?.addEventListener("click",()=>{a.offset+=a.limit,o()});const _=e.querySelector("#audit-export-btn");_?.addEventListener("click",async()=>{_.disabled=!0,_.textContent="导出中...";try{const u=await b.exportAuditLogs({log_type:a.logType,device_id:a.deviceId,status:a.status,keyword:a.keyword}),h=URL.createObjectURL(u),x=document.createElement("a");x.href=h,x.download="multinteractor-audit-export.json",document.body.appendChild(x),x.click(),document.body.removeChild(x),URL.revokeObjectURL(h),v("已导出审计日志 JSON 文件","success")}catch(u){v(u instanceof w?u.message:"审计日志导出失败","error")}finally{_.disabled=!1,_.textContent="导出审计 JSON"}})}function N(e,t,s,a){const o=s.searchQuery.toLowerCase(),r=t.filter(i=>s.onlineOnly&&!i.online?!1:o?i.device_id.toLowerCase().includes(o)||i.device_name.toLowerCase().includes(o)||i.os.toLowerCase().includes(o)||(i.lan_endpoint??"").toLowerCase().includes(o):!0),d=r.length===0?'<tr><td colspan="6" class="py-10 text-center text-slate-500 text-xs">未匹配到任何已注册设备</td></tr>':r.map(i=>{const c=i.enabled?i.online?"bg-sky-500/20 text-sky-400":"bg-slate-800 text-slate-400":"bg-rose-500/20 text-rose-400",m=(i.device_name||i.device_id||"?").slice(0,1).toUpperCase(),g=i.acl_restricted?`ACL 限制 (${i.allowed_categories.join(",")})`:`Sub: ${i.subscribed_topics.length} 主题 • 全类型允许`,$=i.enabled?i.online?`<span class="w-2 h-2 rounded-full bg-emerald-400 animate-pulse"></span><span class="text-emerald-400 font-medium">在线 (${n(D(i.last_seen_ms))})</span>`:`<span class="w-2 h-2 rounded-full bg-slate-600"></span><span class="text-slate-400">离线 (${n(D(i.last_seen_ms))})</span>`:'<span class="w-2 h-2 rounded-full bg-rose-500"></span><span class="text-rose-400 font-medium">已禁用</span>',_=i.enabled?'<span class="px-2 py-0.5 rounded-full bg-emerald-500/10 text-emerald-400 border border-emerald-500/20 text-[11px]">准入已启用</span>':'<span class="px-2 py-0.5 rounded-full bg-rose-500/10 text-rose-400 border border-rose-500/20 text-[11px]">已禁止接入</span>';return`
              <tr class="hover:bg-slate-900/50 transition ${i.online?"":"opacity-80"}">
                <td class="py-3.5 px-4">
                  <div class="flex items-center space-x-2.5">
                    <div class="w-7 h-7 rounded-lg ${c} flex items-center justify-center font-bold text-xs shrink-0">${n(m)}</div>
                    <div>
                      <div class="font-semibold text-white flex items-center space-x-1.5">
                        <span>${n(i.device_name)}</span>
                        <span class="text-[10px] px-1.5 py-0.5 rounded bg-slate-800 text-slate-400 font-mono">${n(i.device_id)}</span>
                      </div>
                      <div class="text-[11px] ${i.acl_restricted?"text-amber-300":"text-slate-400"} font-mono mt-0.5">${n(g)}</div>
                    </div>
                  </div>
                </td>
                <td class="py-3.5 px-4 font-mono">
                  <span class="inline-flex items-center space-x-1.5 text-slate-300">${ee(i.os)}<span>${n(i.os)}</span></span>
                </td>
                <td class="py-3.5 px-4 font-mono text-slate-300">
                  ${i.lan_endpoint?`<span class="px-2 py-1 rounded bg-slate-900 border border-slate-800 text-sky-300">${n(i.lan_endpoint)}</span>`:'<span class="text-slate-500">未上报</span>'}
                </td>
                <td class="py-3.5 px-4">
                  <div class="flex items-center space-x-1.5">${$}</div>
                  <div class="text-[10px] text-slate-500 font-mono mt-0.5">首次: ${n(j(i.first_seen_ms))}</div>
                </td>
                <td class="py-3.5 px-4">${_}</td>
                <td class="py-3.5 px-4 text-right space-x-2 whitespace-nowrap">
                  <button data-action="acl" data-id="${n(i.device_id)}" class="dev-act text-sky-400 hover:text-sky-300 text-xs font-medium">配置 ACL</button>
                  ${i.online?`<button data-action="kick" data-id="${n(i.device_id)}" class="dev-act text-amber-400 hover:text-amber-300 text-xs font-medium">踢出</button>`:""}
                  ${i.enabled?`<button data-action="disable" data-id="${n(i.device_id)}" class="dev-act text-rose-400 hover:text-rose-300 text-xs font-medium">禁用</button>`:`<button data-action="enable" data-id="${n(i.device_id)}" class="dev-act text-emerald-400 hover:text-emerald-300 text-xs font-medium">启用</button>`}
                  <button data-action="remove" data-id="${n(i.device_id)}" class="dev-act text-slate-500 hover:text-rose-400 text-xs font-medium">注销</button>
                </td>
              </tr>
            `}).join("");e.innerHTML=`
    <div class="space-y-6">
      <div class="flex flex-col sm:flex-row sm:items-center justify-between gap-4">
        <div>
          <h2 class="text-lg font-bold text-white">已注册设备台账 (Device Registry)</h2>
          <p class="text-xs text-slate-400">SQLite 持久化设备台账、内网通信端点映射、在线连接踢出与内容分类 ACL 控制</p>
        </div>
        <div class="flex flex-wrap items-center gap-3">
          <label class="inline-flex items-center space-x-1.5 text-xs text-slate-300 cursor-pointer">
            <input type="checkbox" id="devices-online-only" ${s.onlineOnly?"checked":""} class="rounded bg-slate-900 border-slate-700 text-sky-500" />
            <span>仅显示在线</span>
          </label>
          <input type="text" id="devices-search-input" value="${n(s.searchQuery)}" placeholder="搜索设备名称 / ID / IP..." class="bg-slate-950 border border-slate-800 rounded-lg px-3 py-1.5 text-xs text-slate-200 placeholder-slate-500 focus:outline-none focus:border-sky-500 w-52" />
          <button id="devices-issue-btn" class="px-3 py-1.5 rounded-lg text-xs font-medium bg-sky-500 hover:bg-sky-400 text-white transition flex items-center space-x-1.5 shadow-sm">
            <svg class="w-3.5 h-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 4v16m8-8H4" /></svg>
            <span>准入新设备 (签发 Token)</span>
          </button>
        </div>
      </div>

      <div class="bg-slate-950/70 border border-slate-800 rounded-xl overflow-x-auto shadow-sm">
        <table class="w-full text-left text-xs text-slate-300">
          <thead class="bg-slate-900/90 text-slate-400 border-b border-slate-800 uppercase font-mono text-[11px]">
            <tr>
              <th class="py-3 px-4">设备信息 (ID / Name)</th>
              <th class="py-3 px-4">操作系统</th>
              <th class="py-3 px-4">内网通信端点 (LAN)</th>
              <th class="py-3 px-4">在线状态 / 首次上线</th>
              <th class="py-3 px-4">准入状态</th>
              <th class="py-3 px-4 text-right">管理操作</th>
            </tr>
          </thead>
          <tbody class="divide-y divide-slate-800 font-sans">${d}</tbody>
        </table>
      </div>
    </div>
  `;const l=e.querySelector("#devices-search-input");l.addEventListener("input",()=>{s.searchQuery=l.value,N(e,t,s,a);const i=e.querySelector("#devices-search-input");i&&(i.focus(),i.setSelectionRange(i.value.length,i.value.length))});const p=e.querySelector("#devices-online-only");p.addEventListener("change",()=>{s.onlineOnly=p.checked,N(e,t,s,a)}),e.querySelector("#devices-issue-btn")?.addEventListener("click",()=>{W(()=>a())}),e.querySelectorAll(".dev-act").forEach(i=>{i.addEventListener("click",async()=>{const c=i.dataset.action,m=i.dataset.id;if(!c||!m)return;const g=t.find($=>$.device_id===m);if(g){if(c==="acl"){G(g,()=>a());return}try{if(c==="kick")await b.kickDevice(m),v(`已踢出设备 ${g.device_name} 的当前连接`,"info");else if(c==="disable")await b.disableDevice(m),v(`已禁用设备 ${g.device_name}`,"warn");else if(c==="enable")await b.enableDevice(m),v(`已恢复启用设备 ${g.device_name}`,"success");else if(c==="remove"){if(!window.confirm(`确认从 SQLite 台账中彻底注销设备「${g.device_name} (${m})」？`))return;await b.removeDevice(m),v(`已彻底移除设备 ${g.device_name}`,"warn")}a()}catch($){v($ instanceof w?$.message:"设备操作失败","error")}}})})}function oe(e){const r=e.devices.slice(0,12);if(r.length===0)return`
      <div class="relative h-64 bg-slate-900/60 rounded-lg border border-slate-800/80 overflow-hidden flex items-center justify-center">
        <svg class="w-full h-full" viewBox="0 0 620 260">
          <defs>
            <linearGradient id="gradHub" x1="0%" y1="0%" x2="100%" y2="100%">
              <stop offset="0%" stop-color="#0284c7" />
              <stop offset="100%" stop-color="#4f46e5" />
            </linearGradient>
          </defs>
          <g transform="translate(310, 130)">
            <circle r="38" fill="url(#gradHub)" />
            <circle r="38" fill="none" stroke="#38bdf8" stroke-width="1.5" class="animate-pulse-subtle" />
            <text y="-4" text-anchor="middle" fill="#ffffff" font-size="11" font-weight="bold">Cloud Hub</text>
            <text y="12" text-anchor="middle" fill="#bae6fd" font-size="9" font-family="monospace">FastAPI / WAL</text>
          </g>
          <text x="310" y="220" text-anchor="middle" fill="#64748b" font-size="11">暂无已注册设备，等待 mi-daemon 接入信令网关 /ws/signaling ...</text>
        </svg>
      </div>
    `;const d=new Map,l=r.length;r.forEach((u,h)=>{const x=-Math.PI*.75+h*2*Math.PI/l,k=Math.round(310+205*Math.cos(x)),S=Math.round(130+85*Math.sin(x));d.set(u.device_id,{x:k,y:S,dev:u})});const p=Array.from(d.values()).map(({x:u,y:h,dev:x})=>x.enabled?x.online?`<line x1="310" y1="130" x2="${u}" y2="${h}" stroke="#0284c7" stroke-width="1.6" stroke-opacity="0.65" />`:`<line x1="310" y1="130" x2="${u}" y2="${h}" stroke="#475569" stroke-width="1" stroke-dasharray="4 4" stroke-opacity="0.45" />`:`<line x1="310" y1="130" x2="${u}" y2="${h}" stroke="#f43f5e" stroke-width="1.2" stroke-dasharray="3 3" stroke-opacity="0.55" />`).join(""),i=[],c=new Set,m=new Set,g=new Set;for(const u of e.channel_activity){const h=[u.source_device_id,u.target_device_id].sort().join("|");u.channel_used==="LAN_DIRECT"?m.add(h):u.channel_used==="CLOUD_CACHE"&&g.add(h)}for(const[u,h]of Object.entries(e.lan_subnets))for(let x=0;x<h.length;x++)for(let k=x+1;k<h.length;k++){const S=h[x],E=h[k];if(!S||!E)continue;const T=d.get(S),L=d.get(E);if(!T||!L)continue;const M=[S,E].sort().join("|");if(c.has(M))continue;c.add(M);const q=Math.round((T.x+L.x)/2),H=Math.round((T.y+L.y)/2),P=Math.round(q+(q-310)*.25),z=Math.round(H+(H-130)*.25),I=m.has(M),A=!I&&g.has(M),V=A?"#f59e0b":"#10b981",Q=I?"LAN_DIRECT":A?"CLOUD FALLBACK":`LAN ${u}`;i.push(`<path d="M ${T.x} ${T.y} Q ${P} ${z} ${L.x} ${L.y}" fill="none" stroke="${V}" stroke-width="${I||A?"2.2":"1.4"}" stroke-dasharray="5 3" stroke-opacity="${I||A?"0.9":"0.5"}" />`,`<text x="${P}" y="${z-4}" text-anchor="middle" fill="${V}" font-size="9" font-family="monospace">${n(Q)}</text>`)}const $=[];for(const u of e.active_streams){const h=d.get(u.source_device_id);if(h){const x=Math.round((h.x+310)/2),k=Math.round((h.y+130)/2)-18;$.push(`<path d="M ${h.x} ${h.y} Q ${x} ${k} 310 130" fill="none" stroke="#818cf8" stroke-width="2.6" class="stream-flow-active" />`)}for(const x of u.active_readers){const k=d.get(x.reader_device_id);if(k){const S=Math.round((310+k.x)/2),E=Math.round((130+k.y)/2)-18;$.push(`<path d="M 310 130 Q ${S} ${E} ${k.x} ${k.y}" fill="none" stroke="#38bdf8" stroke-width="2.4" class="stream-flow-active" />`)}}}const _=Array.from(d.values()).map(({x:u,y:h,dev:x})=>{const k=x.enabled?x.online?"#38bdf8":"#475569":"#f43f5e",S=x.enabled?x.online?"#10b981":"#64748b":"#f43f5e",E=x.online&&x.enabled?"1":"0.65",T=x.device_name.length>11?`${x.device_name.slice(0,10)}…`:x.device_name,L=x.lan_endpoint?x.lan_endpoint.split(":")[0]??x.lan_endpoint:x.online?"WSS Only":"离线",M=`${te(x.os)} • ${L}`;return`
        <g transform="translate(${u}, ${h})" class="cursor-pointer topo-device-node" data-device-id="${n(x.device_id)}" opacity="${E}">
          <rect x="-54" y="-22" width="108" height="44" rx="8" fill="#0f172a" stroke="${k}" stroke-width="1.5" />
          <circle cx="-40" cy="0" r="4" fill="${S}" />
          <text x="-30" y="-3" fill="#e2e8f0" font-size="10" font-weight="600">${n(T)}</text>
          <text x="-30" y="11" fill="#94a3b8" font-size="8" font-family="monospace">${n(M)}</text>
        </g>
      `}).join("");return`
    <div class="relative h-64 bg-slate-900/60 rounded-lg border border-slate-800/80 overflow-hidden flex items-center justify-center">
      <svg class="w-full h-full" viewBox="0 0 620 260">
        <defs>
          <linearGradient id="gradHub" x1="0%" y1="0%" x2="100%" y2="100%">
            <stop offset="0%" stop-color="#0284c7" />
            <stop offset="100%" stop-color="#4f46e5" />
          </linearGradient>
        </defs>
        ${p}
        ${i.join("")}
        ${$.join("")}
        <g transform="translate(310, 130)">
          <circle r="36" fill="url(#gradHub)" filter="drop-shadow(0 0 12px rgba(2, 132, 199, 0.38))" />
          <circle r="36" fill="none" stroke="#38bdf8" stroke-width="1.5" class="animate-pulse-subtle" />
          <text y="-4" text-anchor="middle" fill="#ffffff" font-size="11" font-weight="bold">Cloud Hub</text>
          <text y="12" text-anchor="middle" fill="#bae6fd" font-size="9" font-family="monospace">${n(e.system.journal_mode)} / v6</text>
        </g>
        ${_}
      </svg>
    </div>
  `}function le(e,t){if(!e)return`
      <div class="bg-slate-900/80 border border-slate-800 rounded-lg p-4 text-xs text-slate-400 space-y-2">
        <div class="font-semibold text-slate-200">当前无活跃流式中转会话</div>
        <p class="leading-relaxed text-slate-400">
          局域网同网段默认由 Rust 客户端开启即用即连内网直传（空闲 ${t} 秒客户端自主关闭）；跨网段或内网异常时自动无缝切换云端 Stream Teeing 边传边下缓存（24h 超时自动清理）。
        </p>
      </div>
    `;const s=e.active_readers[0],a=s?s.progress_pct:e.state==="COMPLETED"?100:0,o=s?s.read_offset:e.state==="COMPLETED"?e.total_size:0,r=e.active_readers.length>0?e.active_readers.map(l=>n(l.reader_device_id)).join(", "):e.state==="COMPLETED"?`已落盘 (${e.download_count} 次拉取)`:"等待接收端接入",d=e.state==="STREAMING"?'<span class="text-xs px-2 py-0.5 rounded bg-emerald-500/10 text-emerald-400 border border-emerald-500/20 font-mono">边传边下中</span>':e.state==="COMPLETED"?'<span class="text-xs px-2 py-0.5 rounded bg-sky-500/10 text-sky-400 border border-sky-500/20 font-mono">已落盘归档</span>':'<span class="text-xs px-2 py-0.5 rounded bg-rose-500/10 text-rose-400 border border-rose-500/20 font-mono">ABORTED</span>';return`
    <div class="bg-slate-900 border border-slate-800 rounded-lg p-3.5 space-y-3">
      <div class="flex items-start justify-between gap-2">
        <div class="min-w-0">
          <div class="text-xs font-semibold text-slate-200 truncate">${n(e.file_name)}</div>
          <div class="text-[11px] text-slate-400 mt-0.5 truncate">
            源端: <span class="text-sky-300 font-mono">${n(e.source_device_id)}</span>
            → 接收端: <span class="text-indigo-300 font-mono">${r}</span>
          </div>
        </div>
        ${d}
      </div>

      <div>
        <div class="flex items-center justify-between text-xs mb-1">
          <span class="text-slate-400">已上传落盘 (Upload-Offset)</span>
          <span class="font-mono text-sky-400">${f(e.written_offset)} / ${f(e.total_size)} (${e.progress_pct.toFixed(1)}%)</span>
        </div>
        <div class="w-full bg-slate-800 rounded-full h-2 overflow-hidden">
          <div class="bg-sky-500 h-2 rounded-full transition-all duration-300" style="width: ${e.progress_pct}%"></div>
        </div>

        <div class="flex items-center justify-between text-xs mt-2.5 mb-1">
          <span class="text-slate-400">接收端消费水位 (Range Read)</span>
          <span class="font-mono text-indigo-300">${f(o)} / ${f(e.total_size)} (${a.toFixed(1)}%)</span>
        </div>
        <div class="w-full bg-slate-800 rounded-full h-2 overflow-hidden">
          <div class="bg-indigo-400 h-2 rounded-full transition-all duration-300" style="width: ${a}%"></div>
        </div>
      </div>

      <div class="pt-2 border-t border-slate-800/80 grid grid-cols-2 gap-2 text-[11px]">
        <div>
          <span class="text-slate-500">4MB 分块校验:</span>
          <div class="text-slate-300 font-mono">${e.verified_chunks} / ${e.total_chunks} 块已验</div>
        </div>
        <div>
          <span class="text-slate-500">24H 缓存剩余 TTL:</span>
          <div class="text-amber-300 font-mono" data-ttl-seconds="${e.ttl_remaining_seconds}">${O(e.ttl_remaining_seconds)}</div>
        </div>
      </div>
    </div>
  `}function ie(e){const t=Z(e.timestamp_ms);if(e.record_type==="EVENT")return`
      <tr class="hover:bg-slate-900/40 cursor-pointer audit-jump-row">
        <td class="py-2 px-3 text-slate-400">${n(t)}</td>
        <td class="py-2 px-3"><span class="px-2 py-0.5 rounded bg-sky-500/10 text-sky-400">EVENT</span></td>
        <td class="py-2 px-3 text-white">${n(e.source_device_id)}</td>
        <td class="py-2 px-3 text-slate-300">${n(e.topic)}</td>
        <td class="py-2 px-3 text-emerald-400">${n(e.content_category)} (${f(e.size_bytes??0)}) • ${n(B(e.content_hash))}</td>
        <td class="py-2 px-3 text-slate-400">投递 ${(e.target_device_ids??[]).length} 台设备</td>
      </tr>
    `;if(e.record_type==="TRANSFER"){const a=e.verification_status==="VERIFIED_OK",o=e.channel_used==="CLOUD_CACHE";return`
      <tr class="hover:bg-slate-900/40 cursor-pointer audit-jump-row">
        <td class="py-2 px-3 text-slate-400">${n(t)}</td>
        <td class="py-2 px-3"><span class="px-2 py-0.5 rounded bg-indigo-500/10 text-indigo-400">TRANSFER</span></td>
        <td class="py-2 px-3 text-white">${n(e.source_device_id)} → ${n(e.target_device_id)}</td>
        <td class="py-2 px-3 text-slate-300">${n(e.file_cache_id||e.transfer_id)}</td>
        <td class="py-2 px-3 ${a?"text-emerald-400":"text-rose-400"}">${n(e.verification_status)} (${f(e.bytes_transferred??0)})</td>
        <td class="py-2 px-3 ${o?"text-indigo-400":"text-emerald-400"}">${n(e.channel_used)}${e.resumed?` • 续传#${e.resume_count}`:""}</td>
      </tr>
    `}const s=e.status==="OK";return`
    <tr class="hover:bg-slate-900/40 cursor-pointer audit-jump-row">
      <td class="py-2 px-3 text-slate-400">${n(t)}</td>
      <td class="py-2 px-3"><span class="px-2 py-0.5 rounded bg-emerald-500/10 text-emerald-400">CONN</span></td>
      <td class="py-2 px-3 text-white">${n(e.device_name||e.device_id)}</td>
      <td class="py-2 px-3 text-slate-300">${n(e.action)}</td>
      <td class="py-2 px-3 ${s?"text-emerald-400":"text-rose-400"}">${n(e.status)}</td>
      <td class="py-2 px-3 text-slate-400 truncate max-w-xs">${n(e.lan_endpoint||e.detail||"-")}</td>
    </tr>
  `}function re(e,t,s){const a=t.cache_quota.usage_pct,o=a>=90?"from-rose-500 to-red-600":a>=70?"from-amber-400 to-orange-500":"from-sky-500 to-indigo-500",r=t.active_streams[0]??t.recent_sessions[0],d=t.cache_quota.active_reader_count,l=`Win ${t.device_stats.online_by_os.windows} • Mac ${t.device_stats.online_by_os.macos} • Lnx ${t.device_stats.online_by_os.linux}`,p=t.security_warnings.map(i=>`
      <div class="rounded-xl border ${i.severity==="HIGH"?"bg-rose-500/10 border-rose-500/35 text-rose-200":"bg-amber-500/10 border-amber-500/35 text-amber-200"} px-4 py-3 flex flex-col sm:flex-row sm:items-center justify-between gap-3 text-xs">
        <div class="flex items-start space-x-2.5">
          <span class="px-1.5 py-0.5 rounded font-mono text-[10px] font-bold shrink-0 ${i.severity==="HIGH"?"bg-rose-500/20 text-rose-300":"bg-amber-500/20 text-amber-300"}">${n(i.severity)}</span>
          <div>
            <div class="font-semibold">${n(i.title)}</div>
            <div class="opacity-85 mt-0.5">${n(i.message)}</div>
          </div>
        </div>
        <button class="goto-security-btn shrink-0 px-3 py-1 rounded-lg bg-slate-950/60 hover:bg-slate-900 border border-current/20 font-medium">
          前往安全中心处理 →
        </button>
      </div>
    `).join("");e.innerHTML=`
    <div class="space-y-6">
      ${p}

      <!-- 4 KPI Cards -->
      <div class="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-4 gap-4">
        <div class="bg-slate-950/70 border border-slate-800 rounded-xl p-4 shadow-sm hover:border-slate-700 transition">
          <div class="flex items-center justify-between text-slate-400 text-xs font-medium">
            <span>在线终端设备</span>
            <span class="p-1 rounded bg-sky-500/10 text-sky-400">
              <svg class="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9.75 17L9 20l-1 1h8l-1-1-.75-3M3 13h18M5 17h14a2 2 0 002-2V5a2 2 0 00-2-2H5a2 2 0 00-2 2v10a2 2 0 002 2z" /></svg>
            </span>
          </div>
          <div class="mt-2 flex items-baseline space-x-2">
            <span class="text-2xl font-bold font-mono text-white">${t.device_stats.online}</span>
            <span class="text-xs text-slate-400">/ ${t.device_stats.total} 台账 ${t.device_stats.disabled>0?`(禁用 ${t.device_stats.disabled})`:""}</span>
          </div>
          <div class="mt-2 flex items-center text-xs text-emerald-400 space-x-1.5 font-mono">
            <span class="w-1.5 h-1.5 rounded-full bg-emerald-400"></span>
            <span>${n(l)}</span>
          </div>
        </div>

        <div class="bg-slate-950/70 border border-slate-800 rounded-xl p-4 shadow-sm hover:border-slate-700 transition">
          <div class="flex items-center justify-between text-slate-400 text-xs font-medium">
            <span>边传边下分流会话</span>
            <span class="p-1 rounded bg-indigo-500/10 text-indigo-400">
              <svg class="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M13 10V3L4 14h7v7l9-11h-7z" /></svg>
            </span>
          </div>
          <div class="mt-2 flex items-baseline space-x-2">
            <span class="text-2xl font-bold font-mono text-indigo-300">${t.cache_quota.streaming_count}</span>
            <span class="text-xs text-slate-400">活跃中 / ${t.cache_quota.completed_count} 归档</span>
          </div>
          <div class="mt-2 text-xs text-indigo-400/90 flex items-center justify-between">
            <span>并发消费者: <strong class="text-indigo-200 font-mono">${d}</strong></span>
            <span class="text-[11px] px-1.5 py-0.5 rounded bg-indigo-500/15 text-indigo-300 font-mono">24h 自动清理</span>
          </div>
        </div>

        <div class="bg-slate-950/70 border border-slate-800 rounded-xl p-4 shadow-sm hover:border-slate-700 transition">
          <div class="flex items-center justify-between text-slate-400 text-xs font-medium">
            <span>云端流式缓存池</span>
            <span class="p-1 rounded bg-amber-500/10 text-amber-400">
              <svg class="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M4 7v10c0 2 1.5 3 3.5 3h9c2 0 3.5-1 3.5-3V7M4 7c0-2 1.5-3 3.5-3h9c2 0 3.5 1 3.5 3M4 7h16" /></svg>
            </span>
          </div>
          <div class="mt-2 flex items-baseline space-x-2">
            <span class="text-2xl font-bold font-mono text-white">${f(t.cache_quota.used_bytes)}</span>
            <span class="text-xs text-slate-400">/ ${f(t.cache_quota.quota_bytes)} (${a.toFixed(1)}%)</span>
          </div>
          <div class="mt-2 w-full bg-slate-800 rounded-full h-1.5 overflow-hidden">
            <div class="bg-gradient-to-r ${o} h-1.5 rounded-full transition-all" style="width: ${Math.max(2,a)}%"></div>
          </div>
        </div>

        <div class="bg-slate-950/70 border border-slate-800 rounded-xl p-4 shadow-sm hover:border-slate-700 transition">
          <div class="flex items-center justify-between text-slate-400 text-xs font-medium">
            <span>摘要完整性与续传</span>
            <span class="p-1 rounded bg-emerald-500/10 text-emerald-400">
              <svg class="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 12l2 2 4-4m5.618-4.016A11.955 11.955 0 0112 2.944a11.955 11.955 0 01-8.618 3.04A12.02 12.02 0 003 9c0 5.591 3.824 10.29 9 11.622 5.176-1.332 9-6.03 9-11.622 0-1.042-.133-2.052-.382-3.016z" /></svg>
            </span>
          </div>
          <div class="mt-2 flex items-baseline space-x-2">
            <span class="text-2xl font-bold font-mono text-emerald-400">${t.audit_summary.blake3_pass_rate}%</span>
            <span class="text-xs text-slate-400">BLAKE3 校验</span>
          </div>
          <div class="mt-2 text-xs text-slate-400 flex items-center justify-between">
            <span>断点续传: <strong class="text-slate-200 font-mono">${t.audit_summary.resumed_transfer_count} 次</strong></span>
            <span class="${t.audit_summary.hash_mismatch_count===0?"text-emerald-400":"text-rose-400"}">${t.audit_summary.hash_mismatch_count} 损坏</span>
          </div>
        </div>
      </div>

      <!-- Topology & Stream Teeing Tracker -->
      <div class="grid grid-cols-1 lg:grid-cols-3 gap-6">
        <div class="lg:col-span-2 bg-slate-950/70 border border-slate-800 rounded-xl p-5 shadow-sm">
          <div class="flex flex-col sm:flex-row sm:items-center justify-between gap-2 mb-4">
            <div>
              <h3 class="text-sm font-semibold text-white flex items-center space-x-2">
                <span>多机协同与通道拓扑</span>
                <span class="text-xs px-2 py-0.5 rounded bg-sky-500/10 text-sky-400 font-normal">实时信令中枢</span>
              </h3>
              <p class="text-xs text-slate-400 mt-0.5">展示真实 WSS 信令连接、同网段内网直连候选分组（客户端自主控制 180s 空闲断开）及云端 Stream Teeing 兜底中转</p>
            </div>
            <div class="flex items-center space-x-3 text-xs shrink-0">
              <span class="flex items-center space-x-1.5"><span class="w-2.5 h-0.5 bg-sky-400"></span><span class="text-slate-400">WSS 信令</span></span>
              <span class="flex items-center space-x-1.5"><span class="w-2.5 h-0.5 bg-emerald-400"></span><span class="text-slate-400">同网段 LAN</span></span>
              <span class="flex items-center space-x-1.5"><span class="w-2.5 h-0.5 bg-indigo-400"></span><span class="text-slate-400">云端流中转</span></span>
            </div>
          </div>
          ${oe(t)}
        </div>

        <div class="bg-slate-950/70 border border-slate-800 rounded-xl p-5 shadow-sm flex flex-col justify-between">
          <div>
            <div class="flex items-center justify-between mb-3">
              <h3 class="text-sm font-semibold text-white flex items-center space-x-2">
                ${t.active_streams.length>0?'<span class="w-2 h-2 rounded-full bg-indigo-400 animate-ping"></span><span>正在流式传输 (Stream Teeing)</span>':'<span class="w-2 h-2 rounded-full bg-sky-400"></span><span>最近流式缓存会话</span>'}
              </h3>
              ${r?`<span class="text-xs px-2 py-0.5 rounded bg-indigo-500/20 text-indigo-300 font-mono">${n(r.file_cache_id)}</span>`:""}
            </div>
            ${le(r,t.system.lan_idle_timeout_seconds)}
          </div>
          <div class="mt-4 pt-3 border-t border-slate-800/80 flex items-center justify-between text-xs">
            <span class="text-slate-500 font-mono">同步更新时间: ${D(t.timestamp_ms)}</span>
            <button id="overview-goto-stream" class="text-sky-400 hover:text-sky-300 font-medium">查看完整缓存池 →</button>
          </div>
        </div>
      </div>

      <!-- Recent Activity Feed -->
      <div class="bg-slate-950/70 border border-slate-800 rounded-xl p-5 shadow-sm">
        <div class="flex items-center justify-between mb-4">
          <h3 class="text-sm font-semibold text-white flex items-center space-x-2">
            <svg class="w-4 h-4 text-slate-400" fill="none" viewBox="0 0 24 24" stroke="currentColor"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 8v4l3 3m6-3a9 9 0 11-18 0 9 9 0 0118 0z" /></svg>
            <span>实时全链路审计动态流 (SQLite WAL)</span>
          </h3>
          <button id="overview-goto-audit" class="text-xs text-sky-400 hover:underline">查看全部日志</button>
        </div>
        <div class="overflow-x-auto">
          <table class="w-full text-left text-xs text-slate-300">
            <thead class="bg-slate-900/80 text-slate-400 border-b border-slate-800 uppercase font-mono text-[11px]">
              <tr>
                <th class="py-2.5 px-3">时间</th>
                <th class="py-2.5 px-3">类别</th>
                <th class="py-2.5 px-3">设备</th>
                <th class="py-2.5 px-3">主题 / 行为</th>
                <th class="py-2.5 px-3">摘要 / 状态</th>
                <th class="py-2.5 px-3">通道 / 详情</th>
              </tr>
            </thead>
            <tbody class="divide-y divide-slate-800/60 font-mono">
              ${t.recent_activity.length===0?'<tr><td colspan="6" class="py-6 text-center text-slate-500 font-sans">暂无审计日志记录</td></tr>':t.recent_activity.map(ie).join("")}
            </tbody>
          </table>
        </div>
      </div>
    </div>
  `,e.querySelectorAll(".goto-security-btn").forEach(i=>{i.addEventListener("click",()=>s.onSwitchTab("security"))}),e.querySelector("#overview-goto-stream")?.addEventListener("click",()=>s.onSwitchTab("stream")),e.querySelector("#overview-goto-audit")?.addEventListener("click",()=>s.onSwitchTab("audit")),e.querySelectorAll(".audit-jump-row").forEach(i=>{i.addEventListener("click",()=>s.onSwitchTab("audit"))}),e.querySelectorAll(".topo-device-node").forEach(i=>{i.addEventListener("click",()=>{const c=i.dataset.deviceId,m=t.devices.find(g=>g.device_id===c);m&&s.onSelectDevice(m)})})}function de(e,t,s,a,o,r,d){const l=o&&t.cluster_token.plaintext?t.cluster_token.plaintext:t.cluster_token.masked,p=s.map(c=>`
      <div class="rounded-xl border ${c.severity==="HIGH"?"bg-rose-500/10 border-rose-500/35 text-rose-200":"bg-amber-500/10 border-amber-500/35 text-amber-200"} px-4 py-3 text-xs">
        <div class="font-semibold flex items-center space-x-2">
          <span class="px-1.5 py-0.5 rounded font-mono text-[10px] ${c.severity==="HIGH"?"bg-rose-500/25 text-rose-300":"bg-amber-500/25 text-amber-300"}">${n(c.severity)}</span>
          <span>${n(c.title)}</span>
        </div>
        <div class="mt-1 opacity-85">${n(c.message)}</div>
      </div>
    `).join(""),i=t.device_tokens.length===0?'<tr><td colspan="5" class="py-8 text-center text-slate-500 font-sans text-xs">尚未签发设备专属 Token</td></tr>':t.device_tokens.map(c=>`
          <tr class="hover:bg-slate-900/50">
            <td class="py-3 px-4">
              <div class="text-sky-300 font-mono">${n(c.token_prefix)}</div>
              <div class="text-[10px] text-slate-500 font-mono">${n(c.token_id)}</div>
            </td>
            <td class="py-3 px-4 text-slate-400 font-mono">${n(j(c.issued_at_ms))}</td>
            <td class="py-3 px-4 font-sans">
              <div class="text-white">${n(c.label||"未命名设备")}</div>
              ${c.expected_os?`<div class="text-[10px] text-slate-500 font-mono">${n(c.expected_os)}</div>`:""}
            </td>
            <td class="py-3 px-4">
              ${c.revoked?`<span class="px-2 py-0.5 rounded bg-rose-500/10 text-rose-400 text-[10px]">已吊销 (${n(j(c.revoked_at_ms??0))})</span>`:'<span class="px-2 py-0.5 rounded bg-emerald-500/10 text-emerald-400 text-[10px]">有效</span>'}
            </td>
            <td class="py-3 px-4 text-right">
              ${c.revoked?'<span class="text-slate-600 text-xs">已入黑名单</span>':`<button data-token-id="${n(c.token_id)}" class="sec-revoke-btn text-rose-400 hover:text-rose-300 text-xs">吊销</button>`}
            </td>
          </tr>
        `).join("");e.innerHTML=`
    <div class="space-y-6">
      <div>
        <h2 class="text-lg font-bold text-white">安全中心与访问凭据管控 (Security & Credentials)</h2>
        <p class="text-xs text-slate-400">管理双平面密钥隔离（MI_ADMIN_TOKEN vs MI_CLUSTER_TOKEN）、主集群 PSK 轮转与设备专属 Token 吊销黑名单</p>
      </div>

      ${p}

      <div class="bg-slate-950/70 border border-slate-800 rounded-xl p-5 shadow-sm space-y-4">
        <div class="flex flex-col sm:flex-row sm:items-start justify-between gap-3">
          <div>
            <div class="flex items-center space-x-2">
              <h3 class="text-sm font-semibold text-white">主集群预共享密钥 (MI_CLUSTER_TOKEN)</h3>
              <span class="px-2 py-0.5 rounded bg-emerald-500/10 text-emerald-400 text-xs font-mono border border-emerald-500/20">Active</span>
              <span class="px-2 py-0.5 rounded ${a?"bg-sky-500/10 text-sky-400 border-sky-500/20":"bg-amber-500/10 text-amber-300 border-amber-500/20"} text-xs font-mono border">
                ${a?"MI_ADMIN_TOKEN 已隔离":"未配置 MI_ADMIN_TOKEN"}
              </span>
            </div>
            <p class="text-xs text-slate-400 mt-1">仅供客户端守护进程 (mi-daemon) 接入 WebSocket 信令网关与数据缓存通道使用；轮转此密钥不会注销管理控制台会话</p>
          </div>
          <button id="sec-rotate-btn" class="px-3 py-1.5 rounded-lg text-xs font-bold bg-amber-500 hover:bg-amber-400 text-slate-950 transition shrink-0">
            立即轮转集群密钥
          </button>
        </div>

        <div class="flex flex-wrap items-center gap-3 bg-slate-900 border border-slate-800 rounded-lg p-3">
          <span class="text-xs text-slate-500 font-mono">集群密钥:</span>
          <span class="font-mono text-sm text-slate-200 break-all">${n(l)}</span>
          <div class="ml-auto flex items-center space-x-3">
            <button id="sec-toggle-reveal" class="text-xs text-sky-400 hover:text-sky-300">${o?"隐藏明文":"显示明文"}</button>
            <button id="sec-copy-cluster" class="text-xs text-slate-400 hover:text-slate-200">复制</button>
          </div>
        </div>

        <div class="text-[11px] text-slate-400 flex items-center space-x-2">
          <svg class="w-4 h-4 text-emerald-400 shrink-0" fill="none" viewBox="0 0 24 24" stroke="currentColor"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 12l2 2 4-4m5.618-4.016A11.955 11.955 0 0112 2.944a11.955 11.955 0 01-8.618 3.04A12.02 12.02 0 003 9c0 5.591 3.824 10.29 9 11.622 5.176-1.332 9-6.03 9-11.622 0-1.042-.133-2.052-.382-3.016z" /></svg>
          <span>恒定时间验证守卫：${n(t.constant_time_guard)} • SQLite WAL 凭据库仅存储设备 Token SHA-256 摘要</span>
        </div>
      </div>

      <div class="bg-slate-950/70 border border-slate-800 rounded-xl overflow-x-auto shadow-sm">
        <div class="p-4 border-b border-slate-800 flex items-center justify-between">
          <h3 class="text-sm font-semibold text-white">设备专属 Access Token 记录与黑名单</h3>
          <button id="sec-issue-btn" class="text-xs text-sky-400 hover:underline font-medium">+ 签发设备专属 Token</button>
        </div>
        <table class="w-full text-left text-xs text-slate-300">
          <thead class="bg-slate-900/90 text-slate-400 border-b border-slate-800 uppercase font-mono text-[11px]">
            <tr>
              <th class="py-3 px-4">Token 摘要 / ID</th>
              <th class="py-3 px-4">签发时间</th>
              <th class="py-3 px-4">授权设备绑定</th>
              <th class="py-3 px-4">状态</th>
              <th class="py-3 px-4 text-right">操作</th>
            </tr>
          </thead>
          <tbody class="divide-y divide-slate-800 font-mono">${i}</tbody>
        </table>
      </div>
    </div>
  `,e.querySelector("#sec-rotate-btn")?.addEventListener("click",()=>{K(()=>d())}),e.querySelector("#sec-issue-btn")?.addEventListener("click",()=>{W(()=>d())}),e.querySelector("#sec-toggle-reveal")?.addEventListener("click",()=>{r(!o)}),e.querySelector("#sec-copy-cluster")?.addEventListener("click",async()=>{try{const c=o&&t.cluster_token.plaintext?t.cluster_token.plaintext:(await b.getSecurityTokens(!0)).cluster_token.plaintext??"",m=await R(c);v(m?"已复制当前集群 MI_CLUSTER_TOKEN":"复制失败",m?"success":"warn")}catch(c){v(c instanceof w?c.message:"读取密钥失败","error")}}),e.querySelectorAll(".sec-revoke-btn").forEach(c=>{c.addEventListener("click",async()=>{const m=c.dataset.tokenId;if(m)try{await b.revokeToken(m),v(`已吊销设备 Token ${m}`,"warn"),d()}catch(g){v(g instanceof w?g.message:"吊销失败","error")}})})}function ce(e,t,s){e.innerHTML=`
    <div class="space-y-6">
      <div>
        <h2 class="text-lg font-bold text-white">云端中枢运行参数与系统健康 (System Health & Config)</h2>
        <p class="text-xs text-slate-400">查看 CloudHubService 运行环境、SQLite WAL 存储参数、安全平面配置及快捷运维操作</p>
      </div>

      <div class="grid grid-cols-1 md:grid-cols-2 gap-6">
        <div class="bg-slate-950/70 border border-slate-800 rounded-xl p-5 shadow-sm space-y-3">
          <h3 class="text-sm font-semibold text-white">基础服务运行环境</h3>
          <div class="space-y-2 text-xs divide-y divide-slate-800/80">
            <div class="flex items-center justify-between py-1.5">
              <span class="text-slate-400">服务引擎:</span>
              <span class="text-slate-200 font-mono">Python ${n(t.runtime.python_version)} + FastAPI ${n(t.runtime.fastapi_version)}</span>
            </div>
            <div class="flex items-center justify-between py-1.5">
              <span class="text-slate-400">监听绑定地址:</span>
              <span class="text-slate-200 font-mono">${n(t.server.host)}:${t.server.port}</span>
            </div>
            <div class="flex items-center justify-between py-1.5">
              <span class="text-slate-400">持续运行时长:</span>
              <span class="text-emerald-400 font-mono">${n(X(t.uptime_seconds))}</span>
            </div>
            <div class="flex items-center justify-between py-1.5 gap-3">
              <span class="text-slate-400 shrink-0">数据目录 (MI_SERVER_DATA_DIR):</span>
              <span class="text-slate-300 font-mono text-[11px] truncate">${n(t.server.data_dir)}</span>
            </div>
            <div class="flex items-center justify-between py-1.5 gap-3">
              <span class="text-slate-400 shrink-0">SQLite WAL 状态:</span>
              <span class="text-slate-300 font-mono text-[11px]">${n(t.wal.journal_mode)} / SYNC=${n(t.wal.synchronous)} (DB ${f(t.wal.db_size_bytes)} • WAL ${f(t.wal.wal_size_bytes)})</span>
            </div>
            <div class="flex items-center justify-between py-1.5 gap-3">
              <span class="text-slate-400 shrink-0">流式缓存池目录:</span>
              <span class="text-slate-300 font-mono text-[11px] truncate">${n(t.server.cache_dir)}</span>
            </div>
          </div>
        </div>

        <div class="bg-slate-950/70 border border-slate-800 rounded-xl p-5 shadow-sm space-y-3">
          <h3 class="text-sm font-semibold text-white">协同、缓存与安全配额规则</h3>
          <div class="space-y-2 text-xs divide-y divide-slate-800/80">
            <div class="flex items-center justify-between py-1.5">
              <span class="text-slate-400">云端数据缓存 TTL:</span>
              <span class="text-sky-400 font-mono">${t.policies.cache_ttl_seconds}s (${Math.round(t.policies.cache_ttl_seconds/3600)}h 超时自动清理)</span>
            </div>
            <div class="flex items-center justify-between py-1.5">
              <span class="text-slate-400">缓存巡检周期:</span>
              <span class="text-slate-300 font-mono">每 ${t.policies.cache_cleanup_interval_seconds}s 后台扫描</span>
            </div>
            <div class="flex items-center justify-between py-1.5">
              <span class="text-slate-400">缓存配额上限:</span>
              <span class="text-sky-400 font-mono">${f(t.policies.cache_quota_bytes)}</span>
            </div>
            <div class="flex items-center justify-between py-1.5">
              <span class="text-slate-400">内网通道空闲释放 (客户端侧计时):</span>
              <span class="text-emerald-400 font-mono">${t.policies.lan_idle_timeout_seconds}s (Rust 客户端自主控制)</span>
            </div>
            <div class="flex items-center justify-between py-1.5">
              <span class="text-slate-400">审计日志保留期:</span>
              <span class="text-slate-200 font-mono">${t.policies.audit_retention_days} 天</span>
            </div>
            <div class="flex items-center justify-between py-1.5">
              <span class="text-slate-400">分块 BLAKE3 块大小:</span>
              <span class="text-slate-200 font-mono">${f(t.policies.chunk_size_bytes)}</span>
            </div>
          </div>
        </div>
      </div>

      <div class="bg-slate-950/70 border border-slate-800 rounded-xl p-5 shadow-sm space-y-3">
        <h3 class="text-sm font-semibold text-white">运维与诊断快捷操作</h3>
        <div class="flex flex-wrap items-center gap-3">
          <button id="settings-wal-ckpt-btn" class="px-3 py-2 rounded-lg bg-slate-900 border border-slate-800 hover:bg-slate-800 text-xs text-slate-200 transition">
            执行 WAL Checkpoint (TRUNCATE)
          </button>
          <button id="settings-cleanup-audit-btn" class="px-3 py-2 rounded-lg bg-slate-900 border border-slate-800 hover:bg-slate-800 text-xs text-slate-200 transition">
            清理超期 (${t.policies.audit_retention_days} 天前) 审计日志
          </button>
          <a href="/docs" target="_blank" rel="noopener noreferrer" class="px-3 py-2 rounded-lg bg-slate-900 border border-slate-800 hover:bg-slate-800 text-xs text-sky-400 transition inline-flex items-center space-x-1">
            <span>打开 FastAPI OpenAPI (Swagger) 文档</span>
            <svg class="w-3.5 h-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M10 6H6a2 2 0 00-2 2v10a2 2 0 002 2h10a2 2 0 002-2v-4M14 4h6m0 0v6m0-6L10 14" /></svg>
          </a>
        </div>
      </div>
    </div>
  `,e.querySelector("#settings-wal-ckpt-btn")?.addEventListener("click",async()=>{try{const a=await b.runWalCheckpoint();v(`WAL 检查点完成！当前 WAL 体积 ${f(a.wal_size_bytes)}`,"success"),s()}catch(a){v(a instanceof w?a.message:"WAL Checkpoint 执行失败","error")}}),e.querySelector("#settings-cleanup-audit-btn")?.addEventListener("click",async()=>{try{const a=await b.cleanupExpiredAudit();v(`超期审计日志清理完成：已清理 ${a.deleted_rows} 条记录`,"info"),s()}catch(a){v(a instanceof w?a.message:"清理失败","error")}})}function pe(e,t,s,a){const o=t.usage_pct>=90?"bg-rose-500":t.usage_pct>=70?"bg-amber-400":"bg-sky-500",r=s.filter(l=>l.state==="COMPLETED").reduce((l,p)=>l+p.written_offset,0),d=s.length===0?'<tr><td colspan="6" class="py-10 text-center text-slate-500 text-xs font-sans">当前缓存池暂无活跃或保留中的会话文件</td></tr>':s.map(l=>{const p=l.active_readers.length>0?l.active_readers.map(c=>`
                        <div class="text-[11px] bg-indigo-500/10 text-indigo-300 border border-indigo-500/20 rounded px-2 py-0.5 mb-1">
                          ${n(c.reader_device_id)} (${c.progress_pct.toFixed(1)}%)
                        </div>
                      `).join(""):`<span class="text-slate-500">${l.state==="COMPLETED"?`已完成 ${l.download_count} 次拉取`:"等待消费端接入"}</span>`,i=l.state==="STREAMING"?'<span class="px-2 py-0.5 rounded bg-emerald-500/10 text-emerald-400 text-[11px]">STREAMING (边传边下)</span>':l.state==="COMPLETED"?'<span class="px-2 py-0.5 rounded bg-slate-800 text-slate-300 text-[11px]">COMPLETED</span>':'<span class="px-2 py-0.5 rounded bg-rose-500/10 text-rose-400 text-[11px]">ABORTED</span>';return`
              <tr class="hover:bg-slate-900/50 ${l.state==="STREAMING"?"bg-indigo-950/10":""}">
                <td class="py-3 px-4">
                  <div class="font-bold text-sky-400">${n(l.file_cache_id)}</div>
                  <div class="text-[10px] text-slate-500">${n(l.file_name)}</div>
                </td>
                <td class="py-3 px-4">
                  <span class="text-white font-sans font-medium">${n(l.source_device_id)}</span>
                  <div class="text-[11px] text-slate-400 font-sans">${n(l.content_category)} • ${l.verified_chunks}/${l.total_chunks} 块</div>
                </td>
                <td class="py-3 px-4">
                  <div class="flex items-center space-x-2">
                    <span class="text-sky-300">${f(l.written_offset)}</span>
                    <span class="text-slate-500">/ ${f(l.total_size)}</span>
                  </div>
                  <div class="w-36 bg-slate-800 rounded-full h-1.5 mt-1 overflow-hidden">
                    <div class="bg-sky-500 h-1.5 rounded-full" style="width: ${l.progress_pct}%"></div>
                  </div>
                </td>
                <td class="py-3 px-4 font-sans">${p}</td>
                <td class="py-3 px-4">
                  ${i}
                  <div class="text-[10px] text-amber-300 mt-1 font-mono" data-ttl-seconds="${l.ttl_remaining_seconds}">24h TTL 剩余 ${O(l.ttl_remaining_seconds)}</div>
                </td>
                <td class="py-3 px-4 text-right space-x-2 whitespace-nowrap font-sans">
                  <button data-action="detail" data-id="${n(l.file_cache_id)}" class="cache-act text-sky-400 hover:text-sky-300 text-xs">分块摘要</button>
                  ${l.state==="STREAMING"?`<button data-action="abort" data-id="${n(l.file_cache_id)}" class="cache-act text-amber-400 hover:text-amber-300 text-xs">终止</button>`:""}
                  <button data-action="delete" data-id="${n(l.file_cache_id)}" class="cache-act text-rose-400 hover:text-rose-300 text-xs">清除</button>
                </td>
              </tr>
            `}).join("");e.innerHTML=`
    <div class="space-y-6">
      <div class="flex flex-col sm:flex-row sm:items-center justify-between gap-4">
        <div>
          <h2 class="text-lg font-bold text-white">边传边下流式缓存池 (Stream Teeing Pool)</h2>
          <p class="text-xs text-slate-400">一写多读实时分流、Upload-Offset / Range 断点续传与严格 24 小时超时后台自动清理</p>
        </div>
        <div class="flex flex-wrap items-center gap-2.5">
          <button id="cache-prune-parts-btn" class="px-3 py-1.5 rounded-lg text-xs font-medium bg-slate-800 hover:bg-slate-700 text-slate-200 transition border border-slate-700">
            释放孤立 .part 碎片
          </button>
          <button id="cache-prune-all-btn" class="px-3 py-1.5 rounded-lg text-xs font-medium bg-amber-500/15 hover:bg-amber-500/25 text-amber-300 transition border border-amber-500/30 flex items-center space-x-1.5">
            <svg class="w-3.5 h-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M19 7l-.867 12.142A2 2 0 0116.138 21H7.862a2 2 0 01-1.995-1.858L5 7m5 4v6m4-6v6m1-10V4a1 1 0 00-1-1h-4a1 1 0 00-1 1v3M4 7h16" /></svg>
            <span>立即清理 24H 过期缓存</span>
          </button>
        </div>
      </div>

      <div class="grid grid-cols-1 md:grid-cols-3 gap-4">
        <div class="bg-slate-950/70 border border-slate-800 rounded-xl p-4">
          <span class="text-slate-400 text-xs font-medium">当前缓存池占用率</span>
          <div class="mt-2 flex items-baseline justify-between">
            <span class="text-xl font-bold font-mono text-white">${f(t.used_bytes)} <span class="text-xs font-normal text-slate-400">/ ${f(t.quota_bytes)}</span></span>
            <span class="text-xs font-mono font-semibold text-sky-400">${t.usage_pct.toFixed(1)}%</span>
          </div>
          <div class="mt-2 w-full bg-slate-800 rounded-full h-1.5 overflow-hidden">
            <div class="${o} h-1.5 rounded-full" style="width: ${Math.max(1,t.usage_pct)}%"></div>
          </div>
          <div class="mt-2 text-[11px] text-slate-500">剩余配额 ${f(t.remaining_bytes)} • 超出配额按 LRU 淘汰</div>
        </div>

        <div class="bg-slate-950/70 border border-slate-800 rounded-xl p-4">
          <span class="text-slate-400 text-xs font-medium">活跃流式分流管道</span>
          <div class="mt-2 flex items-baseline justify-between">
            <span class="text-xl font-bold font-mono text-indigo-300">${t.streaming_count} 活跃会话</span>
            <span class="text-xs px-2 py-0.5 rounded bg-indigo-500/20 text-indigo-300 font-mono">${t.active_reader_count} 消费者</span>
          </div>
          <div class="mt-2 text-[11px] text-slate-400">支持 1 源端边上传、N 台目标端实时 Range 拉取</div>
        </div>

        <div class="bg-slate-950/70 border border-slate-800 rounded-xl p-4">
          <span class="text-slate-400 text-xs font-medium">24H 留存归档文件</span>
          <div class="mt-2 flex items-baseline justify-between">
            <span class="text-xl font-bold font-mono text-emerald-400">${t.completed_count} 个归档</span>
            <span class="text-xs text-slate-400 font-mono">合计 ${f(r)}</span>
          </div>
          <div class="mt-2 text-[11px] text-slate-400">满 ${Math.round(t.ttl_seconds/3600)}h 后台巡检自动清理销毁</div>
        </div>
      </div>

      <div class="bg-slate-950/70 border border-slate-800 rounded-xl overflow-x-auto shadow-sm">
        <div class="p-4 border-b border-slate-800 flex flex-col sm:flex-row sm:items-center justify-between gap-2">
          <span class="text-xs font-bold uppercase tracking-wider text-slate-300">流式中转会话与落盘清单</span>
          <span class="text-xs text-slate-500 font-mono">目录: ${n(t.cache_dir)}</span>
        </div>
        <table class="w-full text-left text-xs text-slate-300">
          <thead class="bg-slate-900/90 text-slate-400 border-b border-slate-800 uppercase font-mono text-[11px]">
            <tr>
              <th class="py-3 px-4">缓存标识 / 文件</th>
              <th class="py-3 px-4">源设备 / 分块校验</th>
              <th class="py-3 px-4">落盘水位线 (Upload-Offset)</th>
              <th class="py-3 px-4">边传边下消费者</th>
              <th class="py-3 px-4">状态 / 24h TTL 倒计时</th>
              <th class="py-3 px-4 text-right">操作</th>
            </tr>
          </thead>
          <tbody class="divide-y divide-slate-800 font-mono">${d}</tbody>
        </table>
      </div>
    </div>
  `,e.querySelector("#cache-prune-parts-btn")?.addEventListener("click",async()=>{try{const l=await b.pruneCache(!0);v(`孤立碎片清理完成：移除 ${l.removed_sessions} 个中止会话与 ${l.orphan_files_removed} 个文件，释放 ${f(l.freed_bytes)}`,"success"),a()}catch(l){v(l instanceof w?l.message:"碎片清理失败","error")}}),e.querySelector("#cache-prune-all-btn")?.addEventListener("click",async()=>{try{const l=await b.pruneCache(!1);v(`超期与失效缓存清理完成：移除 ${l.removed_sessions} 个会话与 ${l.orphan_files_removed} 个孤立文件，释放 ${f(l.freed_bytes)}`,"success"),a()}catch(l){v(l instanceof w?l.message:"清理失败","error")}}),e.querySelectorAll(".cache-act").forEach(l=>{l.addEventListener("click",async()=>{const p=l.dataset.action,i=l.dataset.id;if(!(!p||!i))try{if(p==="detail"){await se(i);return}if(p==="abort")await b.abortCacheSession(i),v(`已终止缓存会话 ${i}`,"warn");else if(p==="delete"){const c=await b.deleteCacheSession(i);v(`已清除缓存 ${i}，释放 ${f(c.freed_bytes)}`,"success")}a()}catch(c){v(c instanceof w?c.message:"缓存操作失败","error")}})})}class xe{root;activeTab="overview";overview=null;adminTokenIsSeparate=!1;wsConnected=!1;socket=null;deviceViewState={searchQuery:"",onlineOnly:!1};auditFilter={logType:"ALL",deviceId:"",status:"",keyword:"",limit:50,offset:0};revealClusterToken=!1;ttlTickerId=null;initRetryTimerId=null;constructor(t){this.root=t,J(()=>{this.handleLoggedOut()})}async init(){this.initRetryTimerId!==null&&(window.clearTimeout(this.initRetryTimerId),this.initRetryTimerId=null),this.renderLoading("正在验证控制台会话...");try{const t=await b.getSession();if(this.adminTokenIsSeparate=t.admin_token_is_separate,!t.authenticated){U(this.root,t,s=>this.handleAuthenticated(s));return}await this.enterDashboard()}catch{this.renderLoading("无法连接 Cloud Hub 服务，3 秒后自动重试...",!0),this.initRetryTimerId=window.setTimeout(()=>{this.init()},3e3)}}renderLoading(t,s=!1){this.root.innerHTML=`
      <div class="min-h-screen flex items-center justify-center bg-slate-950 text-slate-300 text-xs font-mono">
        <div class="flex flex-col items-center space-y-3">
          <div class="flex items-center space-x-3">
            <span class="w-2.5 h-2.5 rounded-full bg-sky-400 animate-ping"></span>
            <span>${n(t)}</span>
          </div>
          ${s?'<button id="init-retry-btn" class="px-3 py-1.5 rounded-lg bg-slate-900 border border-slate-700 hover:bg-slate-800 text-sky-400 transition">立即重试连接</button>':""}
        </div>
      </div>
    `,s&&this.root.querySelector("#init-retry-btn")?.addEventListener("click",()=>{this.init()})}async handleAuthenticated(t){this.adminTokenIsSeparate=t.admin_token_is_separate,await this.enterDashboard()}handleLoggedOut(){this.socket?.stop(),this.socket=null,this.ttlTickerId!==null&&(window.clearInterval(this.ttlTickerId),this.ttlTickerId=null),b.getSession().then(t=>{this.adminTokenIsSeparate=t.admin_token_is_separate,U(this.root,t,s=>this.handleAuthenticated(s))})}async enterDashboard(){this.overview=await b.getOverview(),this.renderShell(),await this.renderActiveTab(),this.socket?.stop(),this.socket=new Y(t=>{t.type==="telemetry"&&t.overview&&(this.overview=t.overview,this.updateShellBadges(),(this.activeTab==="overview"||this.activeTab==="stream"||this.activeTab==="devices")&&this.renderActiveTab(!0))},t=>{this.wsConnected=t,this.updateShellBadges()}),this.socket.start(),this.ttlTickerId!==null&&window.clearInterval(this.ttlTickerId),this.ttlTickerId=window.setInterval(()=>{document.querySelectorAll("[data-ttl-seconds]").forEach(t=>{const s=Number(t.dataset.ttlSeconds??"0"),a=Math.max(0,s-1);t.dataset.ttlSeconds=String(a);const o=t.textContent?.includes("24h TTL")?"24h TTL 剩余 ":"";t.textContent=`${o}${O(a)}`})},1e3)}renderShell(){const t=this.overview?.system,s=t?`${t.host}:${t.port}`:window.location.host,a=this.overview?.device_stats.total??0,o=(this.overview?.active_streams.length??0)>0,r=[{id:"overview",label:"实时总览大屏",icon:'<path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M4 6a2 2 0 012-2h2a2 2 0 012 2v2a2 2 0 01-2 2H6a2 2 0 01-2-2V6zM14 6a2 2 0 012-2h2a2 2 0 012 2v2a2 2 0 01-2 2h-2a2 2 0 01-2-2V6zM4 16a2 2 0 012-2h2a2 2 0 012 2v2a2 2 0 01-2 2H6a2 2 0 01-2-2v-2zM14 16a2 2 0 012-2h2a2 2 0 012 2v2a2 2 0 01-2 2h-2a2 2 0 01-2-2v-2z" />'},{id:"devices",label:"设备台账与准入",icon:'<path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9.75 17L9 20l-1 1h8l-1-1-.75-3M3 13h18M5 17h14a2 2 0 002-2V5a2 2 0 00-2-2H5a2 2 0 00-2 2v10a2 2 0 002 2z" />',extra:`<span id="nav-device-count" class="ml-auto text-xs px-1.5 py-0.5 rounded-full bg-slate-800 text-slate-300 font-mono">${a}</span>`},{id:"stream",label:"边传边下流式缓存",icon:'<path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M7 16a4 4 0 01-.88-7.903A5 5 0 1115.9 6L16 6a5 5 0 011 9.9M9 19l3 3m0 0l3-3m-3 3V10" />',extra:`<span id="nav-stream-dot" class="ml-auto flex h-2 w-2 relative ${o?"":"hidden"}"><span class="animate-ping absolute inline-flex h-full w-full rounded-full bg-sky-400 opacity-75"></span><span class="relative inline-flex rounded-full h-2 w-2 bg-sky-500"></span></span>`},{id:"audit",label:"全链路日志审计",icon:'<path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 12h6m-6 4h6m2 5H7a2 2 0 01-2-2V5a2 2 0 012-2h5.586a1 1 0 01.707.293l5.414 5.414a1 1 0 01.293.707V19a2 2 0 01-2 2z" />'},{id:"security",label:"安全凭证与 Token",icon:'<path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 15v2m-6 4h12a2 2 0 002-2v-6a2 2 0 00-2-2H6a2 2 0 00-2 2v6a2 2 0 002 2zm10-10V7a4 4 0 00-8 0v4h8z" />'},{id:"settings",label:"系统健康与参数",icon:'<path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M10.325 4.317c.426-1.756 2.924-1.756 3.35 0a1.724 1.724 0 002.573 1.066c1.543-.94 3.31.826 2.37 2.37a1.724 1.724 0 001.065 2.572c1.756.426 1.756 2.924 0 3.35a1.724 1.724 0 00-1.066 2.573c.94 1.543-.826 3.31-2.37 2.37a1.724 1.724 0 00-2.572 1.065c-.426 1.756-2.924 1.756-3.35 0a1.724 1.724 0 00-2.573-1.066c-1.543.94-3.31-.826-2.37-2.37a1.724 1.724 0 00-1.065-2.572c-1.756-.426-1.756-2.924 0-3.35a1.724 1.724 0 001.066-2.573c-.94-1.543.826-3.31 2.37-2.37.996.608 2.296.07 2.572-1.065z" /><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M15 12a3 3 0 11-6 0 3 3 0 016 0z" />'}];this.root.innerHTML=`
      <header class="h-14 border-b border-slate-800 bg-slate-950/85 backdrop-blur px-5 flex items-center justify-between sticky top-0 z-40">
        <div class="flex items-center space-x-3">
          <div class="w-8 h-8 rounded-lg bg-gradient-to-tr from-sky-500 to-indigo-600 flex items-center justify-center shadow-lg shadow-sky-500/20">
            <svg class="w-5 h-5 text-white" fill="none" viewBox="0 0 24 24" stroke="currentColor">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M8 7h12m0 0l-4-4m4 4l-4 4m0 6H4m0 0l4 4m-4-4l4-4" />
            </svg>
          </div>
          <div class="flex items-center space-x-2">
            <span class="font-bold text-base tracking-wide text-white">multinteractor</span>
            <span class="text-xs px-2 py-0.5 rounded-full bg-sky-500/10 text-sky-400 font-mono border border-sky-500/20">Cloud Hub v6</span>
          </div>
          <span class="text-slate-600 text-xs hidden md:inline">|</span>
          <span class="text-slate-400 text-xs hidden md:inline">跨设备协同与流式中转中枢</span>
        </div>

        <div class="flex items-center space-x-3">
          <div class="flex items-center space-x-2 bg-slate-900 border border-slate-800 rounded-lg px-3 py-1.5 text-xs">
            <span id="hdr-ws-dot" class="w-2 h-2 rounded-full ${this.wsConnected?"bg-emerald-500":"bg-amber-400"}"></span>
            <span class="text-slate-300 font-mono">${n(s)}</span>
            <span class="text-slate-600">|</span>
            <span id="hdr-ws-label" class="font-medium ${this.wsConnected?"text-emerald-400":"text-amber-300"}">${this.wsConnected?"TELEMETRY LIVE":"POLLING / CONNECTING"}</span>
          </div>

          <button id="hdr-rotate-btn" class="hidden sm:flex items-center space-x-1.5 px-3 py-1.5 rounded-lg text-xs font-medium bg-amber-500/10 text-amber-300 border border-amber-500/30 hover:bg-amber-500/20 transition">
            <span>轮转集群密钥</span>
          </button>

          <button id="hdr-logout-btn" class="px-2.5 py-1.5 rounded-lg text-xs text-slate-400 hover:text-slate-200 border border-slate-800 hover:bg-slate-900 transition">
            退出登录
          </button>
        </div>
      </header>

      <div class="flex-1 flex overflow-hidden">
        <aside class="w-60 border-r border-slate-800 bg-slate-950/60 p-4 flex flex-col justify-between shrink-0 hidden md:flex">
          <nav class="space-y-1.5" id="sidebar-nav">
            ${r.map(d=>`
              <button data-tab="${d.id}" class="nav-tab-btn w-full flex items-center space-x-3 px-3 py-2.5 rounded-lg text-sm font-medium transition border ${this.activeTab===d.id?"bg-sky-500/15 text-sky-400 border-sky-500/20 shadow-sm":"text-slate-400 hover:text-slate-200 hover:bg-slate-900 border-transparent"}">
                <svg class="w-4 h-4 shrink-0" fill="none" viewBox="0 0 24 24" stroke="currentColor">${d.icon}</svg>
                <span>${d.label}</span>
                ${d.extra??""}
              </button>
            `).join("")}
          </nav>

          <div class="border border-slate-800 rounded-xl p-3 bg-slate-900/60 space-y-2 text-xs">
            <div class="flex items-center justify-between text-slate-400">
              <span>SQLite WAL</span>
              <span class="text-emerald-400 font-mono">${n(t?.synchronous??"NORMAL")}</span>
            </div>
            <div class="flex items-center justify-between text-slate-400">
              <span>客户端内网闲断</span>
              <span class="text-slate-300 font-mono">${t?.lan_idle_timeout_seconds??180}s 策略</span>
            </div>
            <div class="flex items-center justify-between text-slate-400">
              <span>云端缓存 TTL</span>
              <span class="text-sky-400 font-mono">${Math.round((t?.cache_ttl_seconds??86400)/3600)}h 超时清理</span>
            </div>
            <div class="pt-2 border-t border-slate-800/80 flex items-center justify-between">
              <span class="text-slate-500">管理平面隔离</span>
              <span class="font-mono font-semibold ${this.adminTokenIsSeparate?"text-emerald-400":"text-amber-300"}">${this.adminTokenIsSeparate?"ADMIN_TOKEN":"FALLBACK"}</span>
            </div>
          </div>
        </aside>

        <div class="flex-1 flex flex-col overflow-hidden">
          <!-- Mobile Tab Strip -->
          <div class="md:hidden flex items-center space-x-1 overflow-x-auto px-3 py-2 border-b border-slate-800 bg-slate-950/80">
            ${r.map(d=>`
              <button data-tab="${d.id}" class="nav-tab-btn px-2.5 py-1.5 rounded-lg text-xs whitespace-nowrap border ${this.activeTab===d.id?"bg-sky-500/15 text-sky-400 border-sky-500/20":"text-slate-400 border-transparent"}">${d.label}</button>
            `).join("")}
          </div>

          <main id="tab-content-container" class="flex-1 overflow-y-auto p-6 bg-slate-900/40"></main>
        </div>
      </div>
    `,this.root.querySelectorAll(".nav-tab-btn").forEach(d=>{d.addEventListener("click",()=>{const l=d.dataset.tab;l&&this.switchTab(l)})}),this.root.querySelector("#hdr-rotate-btn")?.addEventListener("click",()=>{K(()=>{this.refreshCurrentView()})}),this.root.querySelector("#hdr-logout-btn")?.addEventListener("click",async()=>{await b.logout(),this.handleLoggedOut()})}updateShellBadges(){const t=document.getElementById("nav-device-count");t&&this.overview&&(t.textContent=String(this.overview.device_stats.total));const s=document.getElementById("nav-stream-dot");s&&this.overview&&s.classList.toggle("hidden",this.overview.active_streams.length===0);const a=document.getElementById("hdr-ws-dot"),o=document.getElementById("hdr-ws-label");a&&o&&(a.className=`w-2 h-2 rounded-full ${this.wsConnected?"bg-emerald-500":"bg-amber-400"}`,o.className=`font-medium ${this.wsConnected?"text-emerald-400":"text-amber-300"}`,o.textContent=this.wsConnected?"WS TELEMETRY LIVE":"RECONNECTING")}async switchTab(t){this.activeTab=t,this.root.querySelectorAll(".nav-tab-btn").forEach(s=>{const a=s.dataset.tab===t;s.closest("#sidebar-nav")?s.className=a?"nav-tab-btn w-full flex items-center space-x-3 px-3 py-2.5 rounded-lg text-sm font-medium transition border bg-sky-500/15 text-sky-400 border-sky-500/20 shadow-sm":"nav-tab-btn w-full flex items-center space-x-3 px-3 py-2.5 rounded-lg text-sm font-medium transition border text-slate-400 hover:text-slate-200 hover:bg-slate-900 border-transparent":s.className=a?"nav-tab-btn px-2.5 py-1.5 rounded-lg text-xs whitespace-nowrap border bg-sky-500/15 text-sky-400 border-sky-500/20":"nav-tab-btn px-2.5 py-1.5 rounded-lg text-xs whitespace-nowrap border text-slate-400 border-transparent"}),await this.renderActiveTab()}async refreshCurrentView(){this.overview=await b.getOverview(),this.updateShellBadges(),await this.renderActiveTab()}async renderActiveTab(t=!1){const s=document.getElementById("tab-content-container");if(!s)return;const a=document.activeElement,o=t&&a instanceof HTMLInputElement&&s.contains(a)&&a.id?a.id:null,r=o&&a instanceof HTMLInputElement?a.selectionStart:null,d=o&&a instanceof HTMLInputElement?a.selectionEnd:null;try{if(this.activeTab==="overview"){const l=this.overview??await b.getOverview();this.overview=l,re(s,l,{onSwitchTab:p=>void this.switchTab(p),onSelectDevice:p=>G(p,()=>void this.refreshCurrentView())})}else if(this.activeTab==="devices"){const l=await b.listDevices(!1);N(s,l.devices,this.deviceViewState,()=>{this.refreshCurrentView()})}else if(this.activeTab==="stream"){const l=await b.listCacheSessions();pe(s,l.quota,l.sessions,()=>{this.refreshCurrentView()})}else if(this.activeTab==="audit"){const[l,p]=await Promise.all([b.queryAuditLogs({log_type:this.auditFilter.logType,device_id:this.auditFilter.deviceId||void 0,status:this.auditFilter.status||void 0,keyword:this.auditFilter.keyword||void 0,limit:this.auditFilter.limit,offset:this.auditFilter.offset}),b.listDevices(!1)]);ne(s,l,p.devices,this.auditFilter,()=>{this.renderActiveTab()})}else if(this.activeTab==="security"){const[l,p]=await Promise.all([b.getSecurityTokens(this.revealClusterToken),b.getOverview()]);this.overview=p,de(s,l,p.security_warnings,this.adminTokenIsSeparate,this.revealClusterToken,i=>{this.revealClusterToken=i,this.renderActiveTab()},()=>void this.refreshCurrentView())}else if(this.activeTab==="settings"){const l=await b.getSystemStatus();ce(s,l,()=>void this.renderActiveTab())}if(o){const l=s.querySelector(`#${CSS.escape(o)}`);l&&(l.focus(),r!==null&&d!==null&&l.setSelectionRange(r,d))}}catch(l){t||v(l instanceof Error?l.message:"视图加载失败","error")}}}function ue(){if(document.querySelector('link[rel="icon"]'))return;const t=`<svg xmlns="${["http","://www.w3.org/2000/svg"].join("")}" viewBox="0 0 32 32"><rect width="32" height="32" rx="8" fill="#0284c7"/><path d="M9 12h14m0 0l-4-4m4 4l-4 4M23 20H9m0 0l4 4m-4-4l4-4" stroke="#fff" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round" fill="none"/></svg>`,s=new Blob([t],{type:"image/svg+xml"}),a=document.createElement("link");a.rel="icon",a.type="image/svg+xml",a.href=URL.createObjectURL(s),document.head.appendChild(a)}ue();const F=document.getElementById("app");F&&new xe(F).init();
