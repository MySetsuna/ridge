var Dt=Array.isArray,br=Array.prototype.indexOf,Te=Array.prototype.includes,vt=Array.from,yr=Object.defineProperty,ge=Object.getOwnPropertyDescriptor,wn=Object.getOwnPropertyDescriptors,mr=Object.prototype,Er=Array.prototype,Ft=Object.getPrototypeOf,Qt=Object.isExtensible;function Ye(e){return typeof e=="function"}const $r=()=>{};function Sr(e){return e()}function $t(e){for(var t=0;t<e.length;t++)e[t]()}function bn(){var e,t,n=new Promise((r,i)=>{e=r,t=i});return{promise:n,resolve:e,reject:t}}function Ar(e,t){if(Array.isArray(e))return e;if(!(Symbol.iterator in e))return Array.from(e);const n=[];for(const r of e)if(n.push(r),n.length===t)break;return n}const x=2,Fe=4,tt=8,jt=1<<24,Z=16,ee=32,be=64,St=128,B=512,C=1024,M=2048,fe=4096,L=8192,U=16384,Oe=32768,At=1<<25,Ce=65536,at=1<<17,Tr=1<<18,Ue=1<<19,yn=1<<20,ie=1<<25,Me=65536,ct=1<<21,De=1<<22,we=1<<23,se=Symbol("$state"),mn=Symbol("legacy props"),kr=Symbol(""),En=Symbol("attributes"),Tt=Symbol("class"),kt=Symbol("style"),Nt=Symbol("text"),pt=new class extends Error{name="StaleReactionError";message="The reaction that called `getAbortSignal()` was re-run or destroyed"},zt=!!globalThis.document?.contentType&&globalThis.document.contentType.includes("xml");function $n(e){throw new Error("https://svelte.dev/e/lifecycle_outside_component")}function Nr(){throw new Error("https://svelte.dev/e/async_derived_orphan")}function Pr(e,t,n){throw new Error("https://svelte.dev/e/each_key_duplicate")}function Cr(e){throw new Error("https://svelte.dev/e/effect_in_teardown")}function Mr(){throw new Error("https://svelte.dev/e/effect_in_unowned_derived")}function Or(e){throw new Error("https://svelte.dev/e/effect_orphan")}function xr(){throw new Error("https://svelte.dev/e/effect_update_depth_exceeded")}function Rr(e){throw new Error("https://svelte.dev/e/props_invalid_value")}function Ir(){throw new Error("https://svelte.dev/e/state_descriptors_fixed")}function Lr(){throw new Error("https://svelte.dev/e/state_prototype_fixed")}function Dr(){throw new Error("https://svelte.dev/e/state_unsafe_mutation")}function Fr(){throw new Error("https://svelte.dev/e/svelte_boundary_reset_onerror")}const jr=1,zr=2,Sn=4,Hr=8,Vr=16,Br=1,Ur=2,An=4,Gr=8,Yr=16,qr=1,Wr=2,P=Symbol("uninitialized"),Tn="http://www.w3.org/1999/xhtml",Kr="http://www.w3.org/2000/svg",Xr="@attach";function Zr(){console.warn("https://svelte.dev/e/derived_inert")}function Jr(){console.warn("https://svelte.dev/e/select_multiple_invalid_value")}function Qr(){console.warn("https://svelte.dev/e/svelte_boundary_reset_noop")}function kn(e){return e===this.v}function ei(e,t){return e!=e?t==t:e!==t||e!==null&&typeof e=="object"||typeof e=="function"}function Nn(e){return!ei(e,this.v)}let Ge=!1;function ti(){Ge=!0}let T=null;function je(e){T=e}function Pn(e,t=!1,n){T={p:T,i:!1,c:null,e:null,s:e,x:null,r:y,l:Ge&&!t?{s:null,u:null,$:[]}:null}}function Cn(e){var t=T,n=t.e;if(n!==null){t.e=null;for(var r of n)Kn(r)}return e!==void 0&&(t.x=e),t.i=!0,T=t.p,e??{}}function nt(){return!Ge||T!==null&&T.l===null}let Re=[];function ni(){var e=Re;Re=[],$t(e)}function ae(e){if(Re.length===0){var t=Re;queueMicrotask(()=>{t===Re&&ni()})}Re.push(e)}function Mn(e){var t=y;if(t===null)return m.f|=we,e;if((t.f&Oe)===0&&(t.f&Fe)===0)throw e;_e(e,t)}function _e(e,t){for(;t!==null;){if((t.f&St)!==0){if((t.f&Oe)===0)throw e;try{t.b.error(e);return}catch(n){e=n}}t=t.parent}throw e}const ri=-7169;function k(e,t){e.f=e.f&ri|t}function Ht(e){(e.f&B)!==0||e.deps===null?k(e,C):k(e,fe)}function On(e){if(e!==null)for(const t of e)(t.f&x)===0||(t.f&Me)===0||(t.f^=Me,On(t.deps))}function xn(e,t,n){(e.f&M)!==0?t.add(e):(e.f&fe)!==0&&n.add(e),On(e.deps),k(e,C)}let st=!1;function ii(e){var t=st;try{return st=!1,[e(),st]}finally{st=t}}let wt=null,xe=null,E=null,Pt=null,J=null,Ct=null,bt=!1,Ie=null,ot=null;var en=0;let si=1;class ye{id=si++;#t=!1;linked=!0;#l=null;#e=null;async_deriveds=new Map;current=new Map;previous=new Map;unblocked=new Set;#o=new Set;#n=new Set;#i=new Set;#r=0;#s=new Map;#d=null;#f=[];#v=[];#h=new Set;#u=new Set;#a=new Map;#c=new Set;is_fork=!1;#_=!1;#E(){if(this.is_fork)return!0;for(const r of this.#s.keys()){for(var t=r,n=!1;t.parent!==null;){if(this.#a.has(t)){n=!0;break}t=t.parent}if(!n)return!0}return!1}skip_effect(t){this.#a.has(t)||this.#a.set(t,{d:[],m:[]}),this.#c.delete(t)}unskip_effect(t,n=r=>this.schedule(r)){var r=this.#a.get(t);if(r){this.#a.delete(t);for(var i of r.d)k(i,M),n(i);for(i of r.m)k(i,fe),n(i)}this.#c.add(t)}#g(){if(this.#t=!0,en++>1e3&&(this.#y(),fi()),!this.#E()){for(const l of this.#h)this.#u.delete(l),k(l,M),this.schedule(l);for(const l of this.#u)k(l,fe),this.schedule(l)}const t=this.#f;this.#f=[],this.apply();var n=Ie=[],r=[],i=ot=[];for(const l of t)try{this.#$(l,n,r)}catch(u){throw Ln(l),u}if(E=null,i.length>0){var s=ye.ensure();for(const l of i)s.schedule(l)}if(Ie=null,ot=null,this.#E()){this.#p(r),this.#p(n);for(const[l,u]of this.#a)In(l,u);i.length>0&&E.#g();return}const f=this.#m();if(f){f.#w(this);return}this.#h.clear(),this.#u.clear();for(const l of this.#o)l(this);this.#o.clear(),Pt=this,tn(r),tn(n),Pt=null,this.#d?.resolve();var o=E;if(this.linked&&this.#r===0&&this.#y(),this.#f.length>0){o===null&&(o=this,this.#b());const l=o;l.#f.push(...this.#f.filter(u=>!l.#f.includes(u)))}o!==null&&o.#g()}#$(t,n,r){t.f^=C;for(var i=t.first;i!==null;){var s=i.f,f=(s&(ee|be))!==0,o=f&&(s&C)!==0,l=o||(s&L)!==0||this.#a.has(i);if(!l&&i.fn!==null){f?i.f^=C:(s&Fe)!==0?n.push(i):it(i)&&((s&Z)!==0&&this.#u.add(i),Be(i));var u=i.first;if(u!==null){i=u;continue}}for(;i!==null;){var a=i.next;if(a!==null){i=a;break}i=i.parent}}}#m(){for(var t=this.#l;t!==null;){if(!t.is_fork){for(const[n,[,r]]of this.current)if(t.current.has(n)&&!r)return t}t=t.#l}return null}#w(t){for(const[r,i]of t.current)!this.previous.has(r)&&t.previous.has(r)&&this.previous.set(r,t.previous.get(r)),this.current.set(r,i);for(const[r,i]of t.async_deriveds){const s=this.async_deriveds.get(r);s&&i.promise.then(s.resolve)}const n=r=>{var i=r.reactions;if(i!==null)for(const o of i){var s=o.f;if((s&x)!==0)n(o);else{var f=o;s&(De|Z)&&!this.async_deriveds.has(f)&&(this.#u.delete(f),k(f,M),this.schedule(f))}}};for(const r of this.current.keys())n(r);this.oncommit(()=>t.discard()),t.#y(),E=this,this.#g()}#p(t){for(var n=0;n<t.length;n+=1)xn(t[n],this.#h,this.#u)}capture(t,n,r=!1){t.v!==P&&!this.previous.has(t)&&this.previous.set(t,t.v),(t.f&we)===0&&(this.current.set(t,[n,r]),J?.set(t,n)),this.is_fork||(t.v=n)}activate(){E=this}deactivate(){E=null,J=null}flush(){try{bt=!0,E=this,this.#g()}finally{en=0,Ct=null,Ie=null,ot=null,bt=!1,E=null,J=null,ke.clear()}}discard(){for(const t of this.#n)t(this);this.#n.clear(),this.#i.clear(),this.#y()}register_created_effect(t){this.#v.push(t)}#S(){this.#y();for(let a=wt;a!==null;a=a.#e){var t=a.id<this.id,n=[];for(const[c,[d,v]]of this.current){if(a.current.has(c)){var r=a.current.get(c)[0];if(t&&d!==r)a.current.set(c,[d,v]);else continue}n.push(c)}if(t)for(const[c,d]of this.async_deriveds){const v=a.async_deriveds.get(c);v&&d.promise.then(v.resolve)}if(a.#t){var i=[...a.current.keys()].filter(c=>!this.current.has(c));if(i.length===0)t&&a.discard();else if(n.length>0){if(t)for(const c of this.#c)a.unskip_effect(c,d=>{(d.f&(Z|De))!==0?a.schedule(d):a.#p([d])});a.activate();var s=new Set,f=new Map;for(var o of n)Rn(o,i,s,f);f=new Map;var l=[...a.current.keys()].filter(c=>this.current.has(c)?this.current.get(c)[0]!==c.v:!0);if(l.length>0)for(const c of this.#v)(c.f&(U|L|at))===0&&Vt(c,l,f)&&((c.f&(De|Z))!==0?(k(c,M),a.schedule(c)):a.#h.add(c));if(a.#f.length>0&&!a.#_){a.apply();for(var u of a.#f)a.#$(u,[],[]);a.#f=[]}a.deactivate()}}}}increment(t,n){if(this.#r+=1,t){let r=this.#s.get(n)??0;this.#s.set(n,r+1)}}decrement(t,n){if(this.#r-=1,t){let r=this.#s.get(n)??0;r===1?this.#s.delete(n):this.#s.set(n,r-1)}this.#_||(this.#_=!0,ae(()=>{this.#_=!1,this.linked&&this.flush()}))}transfer_effects(t,n){for(const r of t)this.#h.add(r);for(const r of n)this.#u.add(r);t.clear(),n.clear()}oncommit(t){this.#o.add(t)}ondiscard(t){this.#n.add(t)}on_fork_commit(t){this.#i.add(t)}run_fork_commit_callbacks(){for(const t of this.#i)t(this);this.#i.clear()}settled(){return(this.#d??=bn()).promise}static ensure(){if(E===null){const t=E=new ye;t.#b(),bt||ae(()=>{t.#t||t.flush()})}return E}apply(){{J=null;return}}schedule(t){if(Ct=t,t.b?.is_pending&&(t.f&(Fe|tt|jt))!==0&&(t.f&Oe)===0){t.b.defer_effect(t);return}for(var n=t;n.parent!==null;){n=n.parent;var r=n.f;if(Ie!==null&&n===y&&(m===null||(m.f&x)===0))return;if((r&(be|ee))!==0){if((r&C)===0)return;n.f^=C}}this.#f.push(n)}#b(){xe===null?wt=xe=this:(xe.#e=this,this.#l=xe),xe=this}#y(){var t=this.#l,n=this.#e;t===null?wt=n:t.#e=n,n===null?xe=t:n.#l=t,this.linked=!1}}function fi(){try{xr()}catch(e){_e(e,Ct)}}let oe=null;function tn(e){var t=e.length;if(t!==0){for(var n=0;n<t;){var r=e[n++];if((r.f&(U|L))===0&&it(r)&&(oe=new Set,Be(r),r.deps===null&&r.first===null&&r.nodes===null&&r.teardown===null&&r.ac===null&&Qn(r),oe?.size>0)){ke.clear();for(const i of oe){if((i.f&(U|L))!==0)continue;const s=[i];let f=i.parent;for(;f!==null;)oe.has(f)&&(oe.delete(f),s.push(f)),f=f.parent;for(let o=s.length-1;o>=0;o--){const l=s[o];(l.f&(U|L))===0&&Be(l)}}oe.clear()}}oe=null}}function Rn(e,t,n,r){if(!n.has(e)&&(n.add(e),e.reactions!==null))for(const i of e.reactions){const s=i.f;(s&x)!==0?Rn(i,t,n,r):(s&(De|Z))!==0&&(s&M)===0&&Vt(i,t,r)&&(k(i,M),Bt(i))}}function Vt(e,t,n){const r=n.get(e);if(r!==void 0)return r;if(e.deps!==null)for(const i of e.deps){if(Te.call(t,i))return!0;if((i.f&x)!==0&&Vt(i,t,n))return n.set(i,!0),!0}return n.set(e,!1),!1}function Bt(e){E.schedule(e)}function In(e,t){if(!((e.f&ee)!==0&&(e.f&C)!==0)){(e.f&M)!==0?t.d.push(e):(e.f&fe)!==0&&t.m.push(e),k(e,C);for(var n=e.first;n!==null;)In(n,t),n=n.next}}function Ln(e){k(e,C);for(var t=e.first;t!==null;)Ln(t),t=t.next}function li(e){let t=0,n=me(0),r;return()=>{qt()&&(N(n),Xn(()=>(t===0&&(r=Ee(()=>e(()=>Je(n)))),t+=1,()=>{ae(()=>{t-=1,t===0&&(r?.(),r=void 0,Je(n))})})))}}var oi=Ce|Ue;function ui(e,t,n,r){new ai(e,t,n,r)}class ai{parent;is_pending=!1;transform_error;#t;#l=null;#e;#o;#n;#i=null;#r=null;#s=null;#d=null;#f=0;#v=0;#h=!1;#u=new Set;#a=new Set;#c=null;#_=li(()=>(this.#c=me(this.#f),()=>{this.#c=null}));constructor(t,n,r,i){this.#t=t,this.#e=n,this.#o=s=>{var f=y;f.b=this,f.f|=St,r(s)},this.parent=y.b,this.transform_error=i??this.parent?.transform_error??(s=>s),this.#n=gt(()=>{this.#m()},oi)}#E(){try{this.#i=F(()=>this.#o(this.#t))}catch(t){this.error(t)}}#g(t){const n=this.#e.failed;n&&(this.#s=F(()=>{n(this.#t,()=>t,()=>()=>{})}))}#$(){const t=this.#e.pending;t&&(this.is_pending=!0,this.#r=F(()=>t(this.#t)),ae(()=>{var n=this.#d=document.createDocumentFragment(),r=ce();n.append(r),this.#i=this.#p(()=>F(()=>this.#o(r))),this.#v===0&&(this.#t.before(n),this.#d=null,Ne(this.#r,()=>{this.#r=null}),this.#w(E))}))}#m(){try{if(this.is_pending=this.has_pending_snippet(),this.#v=0,this.#f=0,this.#i=F(()=>{this.#o(this.#t)}),this.#v>0){var t=this.#d=document.createDocumentFragment();Zt(this.#i,t);const n=this.#e.pending;this.#r=F(()=>n(this.#t))}else this.#w(E)}catch(n){this.error(n)}}#w(t){this.is_pending=!1,t.transfer_effects(this.#u,this.#a)}defer_effect(t){xn(t,this.#u,this.#a)}is_rendered(){return!this.is_pending&&(!this.parent||this.parent.is_rendered())}has_pending_snippet(){return!!this.#e.pending}#p(t){var n=y,r=m,i=T;q(this.#n),Y(this.#n),je(this.#n.ctx);try{return ye.ensure(),t()}catch(s){return Mn(s),null}finally{q(n),Y(r),je(i)}}#S(t,n){if(!this.has_pending_snippet()){this.parent&&this.parent.#S(t,n);return}this.#v+=t,this.#v===0&&(this.#w(n),this.#r&&Ne(this.#r,()=>{this.#r=null}),this.#d&&(this.#t.before(this.#d),this.#d=null))}update_pending_count(t,n){this.#S(t,n),this.#f+=t,!(!this.#c||this.#h)&&(this.#h=!0,ae(()=>{this.#h=!1,this.#c&&He(this.#c,this.#f)}))}get_effect_pending(){return this.#_(),N(this.#c)}error(t){if(!this.#e.onerror&&!this.#e.failed)throw t;E?.is_fork?(this.#i&&E.skip_effect(this.#i),this.#r&&E.skip_effect(this.#r),this.#s&&E.skip_effect(this.#s),E.on_fork_commit(()=>{this.#b(t)})):this.#b(t)}#b(t){this.#i&&(O(this.#i),this.#i=null),this.#r&&(O(this.#r),this.#r=null),this.#s&&(O(this.#s),this.#s=null);var n=this.#e.onerror;let r=this.#e.failed;var i=!1,s=!1;const f=()=>{if(i){Qr();return}i=!0,s&&Fr(),this.#s!==null&&Ne(this.#s,()=>{this.#s=null}),this.#p(()=>{this.#m()})},o=l=>{try{s=!0,n?.(l,f),s=!1}catch(u){_e(u,this.#n&&this.#n.parent)}r&&(this.#s=this.#p(()=>{try{return F(()=>{var u=y;u.b=this,u.f|=St,r(this.#t,()=>l,()=>f)})}catch(u){return _e(u,this.#n.parent),null}}))};ae(()=>{var l;try{l=this.transform_error(t)}catch(u){_e(u,this.#n&&this.#n.parent);return}l!==null&&typeof l=="object"&&typeof l.then=="function"?l.then(o,u=>_e(u,this.#n&&this.#n.parent)):o(l)})}}function Dn(e,t,n,r){const i=nt()?ze:Ut;var s=e.filter(d=>!d.settled);if(n.length===0&&s.length===0){r(t.map(i));return}var f=y,o=ci(),l=s.length===1?s[0].promise:s.length>1?Promise.all(s.map(d=>d.promise)):null;function u(d){if((f.f&U)===0){o();try{r(d)}catch(v){_e(v,f)}dt()}}var a=Fn();if(n.length===0){l.then(()=>u(t.map(i))).finally(a);return}function c(){Promise.all(n.map(d=>di(d))).then(d=>u([...t.map(i),...d])).catch(d=>_e(d,f)).finally(a)}l?l.then(()=>{o(),c(),dt()}):c()}function ci(){var e=y,t=m,n=T,r=E;return function(s=!0){q(e),Y(t),je(n),s&&(e.f&U)===0&&(r?.activate(),r?.apply())}}function dt(e=!0){q(null),Y(null),je(null),e&&E?.deactivate()}function Fn(){var e=y,t=e.b,n=E,r=t.is_rendered();return t.update_pending_count(1,n),n.increment(r,e),()=>{t.update_pending_count(-1,n),n.decrement(r,e)}}function ze(e){var t=x|M;return y!==null&&(y.f|=Ue),{ctx:T,deps:null,effects:null,equals:kn,f:t,fn:e,reactions:null,rv:0,v:P,wv:0,parent:y,ac:null}}const ft=Symbol("obsolete");function di(e,t,n){let r=y;r===null&&Nr();var i=void 0,s=me(P),f=!m,o=new Set;return ki(()=>{var l=y,u=bn();i=u.promise;try{Promise.resolve(e()).then(u.resolve,v=>{v!==pt&&u.reject(v)}).finally(dt)}catch(v){u.reject(v),dt()}var a=E;if(f){if((l.f&Oe)!==0)var c=Fn();if(r.b.is_rendered())a.async_deriveds.get(l)?.reject(ft);else for(const v of o.values())v.reject(ft);o.add(u),a.async_deriveds.set(l,u)}const d=(v,p=void 0)=>{c?.(),o.delete(u),p!==ft&&(a.activate(),p?(s.f|=we,He(s,p)):((s.f&we)!==0&&(s.f^=we),He(s,v)),a.deactivate())};u.promise.then(d,v=>d(null,v||"unknown"))}),_t(()=>{for(const l of o)l.reject(ft)}),new Promise(l=>{function u(a){function c(){a===i?l(s):u(i)}a.then(c,c)}u(i)})}function hi(e){const t=ze(e);return nr(t),t}function Ut(e){const t=ze(e);return t.equals=Nn,t}function vi(e){var t=e.effects;if(t!==null){e.effects=null;for(var n=0;n<t.length;n+=1)O(t[n])}}function Gt(e){var t,n=y,r=e.parent;if(!de&&r!==null&&e.v!==P&&(r.f&(U|L))!==0)return Zr(),e.v;q(r);try{e.f&=~Me,vi(e),t=fr(e)}finally{q(n)}return t}function jn(e){var t=Gt(e);if(!e.equals(t)&&(e.wv=ir(),(!E?.is_fork||e.deps===null)&&(E!==null?(E.capture(e,t,!0),Pt?.capture(e,t,!0)):e.v=t,e.deps===null))){k(e,C);return}de||(J!==null?(qt()||E?.is_fork)&&J.set(e,t):Ht(e))}function pi(e){if(e.effects!==null)for(const t of e.effects)(t.teardown||t.ac)&&(t.teardown?.(),t.ac?.abort(pt),t.fn!==null&&(t.teardown=$r),t.ac=null,Qe(t,0),Kt(t))}function zn(e){if(e.effects!==null)for(const t of e.effects)t.teardown&&t.fn!==null&&Be(t)}let ht=new Set;const ke=new Map;let Hn=!1;function me(e,t){var n={f:0,v:e,reactions:null,equals:kn,rv:0,wv:0};return n}function ve(e,t){const n=me(e);return nr(n),n}function _i(e,t=!1,n=!0){const r=me(e);return t||(r.equals=Nn),Ge&&n&&T!==null&&T.l!==null&&(T.l.s??=[]).push(r),r}function ue(e,t,n=!1){m!==null&&(!Q||(m.f&at)!==0)&&nt()&&(m.f&(x|Z|De|at))!==0&&(G===null||!Te.call(G,e))&&Dr();let r=n?Le(t):t;return He(e,r,ot)}function He(e,t,n=null){if(!e.equals(t)){ke.set(e,de?t:e.v);var r=ye.ensure();if(r.capture(e,t),(e.f&x)!==0){const i=e;(e.f&M)!==0&&Gt(i),J===null&&Ht(i)}e.wv=ir(),Vn(e,M,n),nt()&&y!==null&&(y.f&C)!==0&&(y.f&(ee|be))===0&&(V===null?Ci([e]):V.push(e)),!r.is_fork&&ht.size>0&&!Hn&&gi()}return t}function gi(){Hn=!1;for(const e of ht){(e.f&C)!==0&&k(e,fe);let t;try{t=it(e)}catch{t=!0}t&&Be(e)}ht.clear()}function nn(e,t=1){var n=N(e),r=t===1?n++:n--;return ue(e,n),r}function Je(e){ue(e,e.v+1)}function Vn(e,t,n){var r=e.reactions;if(r!==null)for(var i=nt(),s=r.length,f=0;f<s;f++){var o=r[f],l=o.f;if(!(!i&&o===y)){var u=(l&M)===0;if(u&&k(o,t),(l&at)!==0)ht.add(o);else if((l&x)!==0){var a=o;J?.delete(a),(l&Me)===0&&(l&B&&(y===null||(y.f&ct)===0)&&(o.f|=Me),Vn(a,fe,n))}else if(u){var c=o;(l&Z)!==0&&oe!==null&&oe.add(c),n!==null?n.push(c):Bt(c)}}}}function Le(e){if(typeof e!="object"||e===null||se in e)return e;const t=Ft(e);if(t!==mr&&t!==Er)return e;var n=new Map,r=Dt(e),i=ve(0),s=Pe,f=o=>{if(Pe===s)return o();var l=m,u=Pe;Y(null),ln(s);var a=o();return Y(l),ln(u),a};return r&&n.set("length",ve(e.length)),new Proxy(e,{defineProperty(o,l,u){(!("value"in u)||u.configurable===!1||u.enumerable===!1||u.writable===!1)&&Ir();var a=n.get(l);return a===void 0?f(()=>{var c=ve(u.value);return n.set(l,c),c}):ue(a,u.value,!0),!0},deleteProperty(o,l){var u=n.get(l);if(u===void 0){if(l in o){const a=f(()=>ve(P));n.set(l,a),Je(i)}}else ue(u,P),Je(i);return!0},get(o,l,u){if(l===se)return e;var a=n.get(l),c=l in o;if(a===void 0&&(!c||ge(o,l)?.writable)&&(a=f(()=>{var v=Le(c?o[l]:P),p=ve(v);return p}),n.set(l,a)),a!==void 0){var d=N(a);return d===P?void 0:d}return Reflect.get(o,l,u)},getOwnPropertyDescriptor(o,l){var u=Reflect.getOwnPropertyDescriptor(o,l);if(u&&"value"in u){var a=n.get(l);a&&(u.value=N(a))}else if(u===void 0){var c=n.get(l),d=c?.v;if(c!==void 0&&d!==P)return{enumerable:!0,configurable:!0,value:d,writable:!0}}return u},has(o,l){if(l===se)return!0;var u=n.get(l),a=u!==void 0&&u.v!==P||Reflect.has(o,l);if(u!==void 0||y!==null&&(!a||ge(o,l)?.writable)){u===void 0&&(u=f(()=>{var d=a?Le(o[l]):P,v=ve(d);return v}),n.set(l,u));var c=N(u);if(c===P)return!1}return a},set(o,l,u,a){var c=n.get(l),d=l in o;if(r&&l==="length")for(var v=u;v<c.v;v+=1){var p=n.get(v+"");p!==void 0?ue(p,P):v in o&&(p=f(()=>ve(P)),n.set(v+"",p))}if(c===void 0)(!d||ge(o,l)?.writable)&&(c=f(()=>ve(void 0)),ue(c,Le(u)),n.set(l,c));else{d=c.v!==P;var w=f(()=>Le(u));ue(c,w)}var h=Reflect.getOwnPropertyDescriptor(o,l);if(h?.set&&h.set.call(a,u),!d){if(r&&typeof l=="string"){var _=n.get("length"),A=Number(l);Number.isInteger(A)&&A>=_.v&&ue(_,A+1)}Je(i)}return!0},ownKeys(o){N(i);var l=Reflect.ownKeys(o).filter(c=>{var d=n.get(c);return d===void 0||d.v!==P});for(var[u,a]of n)a.v!==P&&!(u in o)&&l.push(u);return l},setPrototypeOf(){Lr()}})}function rn(e){try{if(e!==null&&typeof e=="object"&&se in e)return e[se]}catch{}return e}function wi(e,t){return Object.is(rn(e),rn(t))}var sn,Bn,Un,Gn;function bi(){if(sn===void 0){sn=window,Bn=/Firefox/.test(navigator.userAgent);var e=Element.prototype,t=Node.prototype,n=Text.prototype;Un=ge(t,"firstChild").get,Gn=ge(t,"nextSibling").get,Qt(e)&&(e[Tt]=void 0,e[En]=null,e[kt]=void 0,e.__e=void 0),Qt(n)&&(n[Nt]=void 0)}}function ce(e=""){return document.createTextNode(e)}function Ve(e){return Un.call(e)}function rt(e){return Gn.call(e)}function yi(e,t){return Ve(e)}function W(e,t=!1){{var n=Ve(e);return n instanceof Comment&&n.data===""?rt(n):n}}function mi(e,t=1,n=!1){let r=e;for(;t--;)r=rt(r);return r}function Ei(e){e.textContent=""}function Yn(){return!1}function qn(e,t,n){return document.createElementNS(t??Tn,e,void 0)}function $i(e,t){if(t){const n=document.body;e.autofocus=!0,ae(()=>{document.activeElement===n&&e.focus()})}}function Yt(e){var t=m,n=y;Y(null),q(null);try{return e()}finally{Y(t),q(n)}}function Wn(e){y===null&&(m===null&&Or(),Mr()),de&&Cr()}function Si(e,t){var n=t.last;n===null?t.last=t.first=e:(n.next=e,e.prev=n,t.last=e)}function te(e,t){var n=y;n!==null&&(n.f&L)!==0&&(e|=L);var r={ctx:T,deps:null,nodes:null,f:e|M|B,first:null,fn:t,last:null,next:null,parent:n,b:n&&n.b,prev:null,teardown:null,wv:0,ac:null};E?.register_created_effect(r);var i=r;if((e&Fe)!==0)Ie!==null?Ie.push(r):ye.ensure().schedule(r);else if(t!==null){try{Be(r)}catch(f){throw O(r),f}i.deps===null&&i.teardown===null&&i.nodes===null&&i.first===i.last&&(i.f&Ue)===0&&(i=i.first,(e&Z)!==0&&(e&Ce)!==0&&i!==null&&(i.f|=Ce))}if(i!==null&&(i.parent=n,n!==null&&Si(i,n),m!==null&&(m.f&x)!==0&&(e&be)===0)){var s=m;(s.effects??=[]).push(i)}return r}function qt(){return m!==null&&!Q}function _t(e){const t=te(tt,null);return k(t,C),t.teardown=e,t}function Mt(e){Wn();var t=y.f,n=!m&&(t&ee)!==0&&(t&Oe)===0;if(n){var r=T;(r.e??=[]).push(e)}else return Kn(e)}function Kn(e){return te(Fe|yn,e)}function Ai(e){return Wn(),te(tt|yn,e)}function Ti(e){ye.ensure();const t=te(be|Ue,e);return(n={})=>new Promise(r=>{n.outro?Ne(t,()=>{O(t),r(void 0)}):(O(t),r(void 0))})}function Wt(e){return te(Fe,e)}function ki(e){return te(De|Ue,e)}function Xn(e,t=0){return te(tt|t,e)}function Es(e,t=[],n=[],r=[]){Dn(r,t,n,i=>{te(tt,()=>e(...i.map(N)))})}function gt(e,t=0){var n=te(Z|t,e);return n}function Zn(e,t=0){var n=te(jt|t,e);return n}function F(e){return te(ee|Ue,e)}function Jn(e){var t=e.teardown;if(t!==null){const n=de,r=m;fn(!0),Y(null);try{t.call(null)}finally{fn(n),Y(r)}}}function Kt(e,t=!1){var n=e.first;for(e.first=e.last=null;n!==null;){const i=n.ac;i!==null&&Yt(()=>{i.abort(pt)});var r=n.next;(n.f&be)!==0?n.parent=null:O(n,t),n=r}}function Ni(e){for(var t=e.first;t!==null;){var n=t.next;(t.f&ee)===0&&O(t),t=n}}function O(e,t=!0){var n=!1;(t||(e.f&Tr)!==0)&&e.nodes!==null&&e.nodes.end!==null&&(Pi(e.nodes.start,e.nodes.end),n=!0),k(e,At),Kt(e,t&&!n),Qe(e,0);var r=e.nodes&&e.nodes.t;if(r!==null)for(const s of r)s.stop();Jn(e),e.f^=At,e.f|=U;var i=e.parent;i!==null&&i.first!==null&&Qn(e),e.next=e.prev=e.teardown=e.ctx=e.deps=e.fn=e.nodes=e.ac=e.b=null}function Pi(e,t){for(;e!==null;){var n=e===t?null:rt(e);e.remove(),e=n}}function Qn(e){var t=e.parent,n=e.prev,r=e.next;n!==null&&(n.next=r),r!==null&&(r.prev=n),t!==null&&(t.first===e&&(t.first=r),t.last===e&&(t.last=n))}function Ne(e,t,n=!0){var r=[];er(e,r,!0);var i=()=>{n&&O(e),t&&t()},s=r.length;if(s>0){var f=()=>--s||i();for(var o of r)o.out(f)}else i()}function er(e,t,n){if((e.f&L)===0){e.f^=L;var r=e.nodes&&e.nodes.t;if(r!==null)for(const o of r)(o.is_global||n)&&t.push(o);for(var i=e.first;i!==null;){var s=i.next;if((i.f&be)===0){var f=(i.f&Ce)!==0||(i.f&ee)!==0&&(e.f&Z)!==0;er(i,t,f?n:!1)}i=s}}}function Xt(e){tr(e,!0)}function tr(e,t){if((e.f&L)!==0){e.f^=L,(e.f&C)===0&&(k(e,M),ye.ensure().schedule(e));for(var n=e.first;n!==null;){var r=n.next,i=(n.f&Ce)!==0||(n.f&ee)!==0;tr(n,i?t:!1),n=r}var s=e.nodes&&e.nodes.t;if(s!==null)for(const f of s)(f.is_global||t)&&f.in()}}function Zt(e,t){if(e.nodes)for(var n=e.nodes.start,r=e.nodes.end;n!==null;){var i=n===r?null:rt(n);t.append(n),n=i}}let ut=!1,de=!1;function fn(e){de=e}let m=null,Q=!1;function Y(e){m=e}let y=null;function q(e){y=e}let G=null;function nr(e){m!==null&&(G===null?G=[e]:G.push(e))}let D=null,j=0,V=null;function Ci(e){V=e}let rr=1,Ae=0,Pe=Ae;function ln(e){Pe=e}function ir(){return++rr}function it(e){var t=e.f;if((t&M)!==0)return!0;if(t&x&&(e.f&=~Me),(t&fe)!==0){for(var n=e.deps,r=n.length,i=0;i<r;i++){var s=n[i];if(it(s)&&jn(s),s.wv>e.wv)return!0}(t&B)!==0&&J===null&&k(e,C)}return!1}function sr(e,t,n=!0){var r=e.reactions;if(r!==null&&!(G!==null&&Te.call(G,e)))for(var i=0;i<r.length;i++){var s=r[i];(s.f&x)!==0?sr(s,t,!1):t===s&&(n?k(s,M):(s.f&C)!==0&&k(s,fe),Bt(s))}}function fr(e){var t=D,n=j,r=V,i=m,s=G,f=T,o=Q,l=Pe,u=e.f;D=null,j=0,V=null,m=(u&(ee|be))===0?e:null,G=null,je(e.ctx),Q=!1,Pe=++Ae,e.ac!==null&&(Yt(()=>{e.ac.abort(pt)}),e.ac=null);try{e.f|=ct;var a=e.fn,c=a();e.f|=Oe;var d=e.deps,v=E?.is_fork;if(D!==null){var p;if(v||Qe(e,j),d!==null&&j>0)for(d.length=j+D.length,p=0;p<D.length;p++)d[j+p]=D[p];else e.deps=d=D;if(qt()&&(e.f&B)!==0)for(p=j;p<d.length;p++)(d[p].reactions??=[]).push(e)}else!v&&d!==null&&j<d.length&&(Qe(e,j),d.length=j);if(nt()&&V!==null&&!Q&&d!==null&&(e.f&(x|fe|M))===0)for(p=0;p<V.length;p++)sr(V[p],e);if(i!==null&&i!==e){if(Ae++,i.deps!==null)for(let w=0;w<n;w+=1)i.deps[w].rv=Ae;if(t!==null)for(const w of t)w.rv=Ae;V!==null&&(r===null?r=V:r.push(...V))}return(e.f&we)!==0&&(e.f^=we),c}catch(w){return Mn(w)}finally{e.f^=ct,D=t,j=n,V=r,m=i,G=s,je(f),Q=o,Pe=l}}function Mi(e,t){let n=t.reactions;if(n!==null){var r=br.call(n,e);if(r!==-1){var i=n.length-1;i===0?n=t.reactions=null:(n[r]=n[i],n.pop())}}if(n===null&&(t.f&x)!==0&&(D===null||!Te.call(D,t))){var s=t;(s.f&B)!==0&&(s.f^=B,s.f&=~Me),s.v!==P&&Ht(s),pi(s),Qe(s,0)}}function Qe(e,t){var n=e.deps;if(n!==null)for(var r=t;r<n.length;r++)Mi(e,n[r])}function Be(e){var t=e.f;if((t&U)===0){k(e,C);var n=y,r=ut;y=e,ut=!0;try{(t&(Z|jt))!==0?Ni(e):Kt(e),Jn(e);var i=fr(e);e.teardown=typeof i=="function"?i:null,e.wv=rr;var s}finally{ut=r,y=n}}}function N(e){var t=e.f,n=(t&x)!==0;if(m!==null&&!Q){var r=y!==null&&(y.f&U)!==0;if(!r&&(G===null||!Te.call(G,e))){var i=m.deps;if((m.f&ct)!==0)e.rv<Ae&&(e.rv=Ae,D===null&&i!==null&&i[j]===e?j++:D===null?D=[e]:D.push(e));else{m.deps??=[],Te.call(m.deps,e)||m.deps.push(e);var s=e.reactions;s===null?e.reactions=[m]:Te.call(s,m)||s.push(m)}}}if(de&&ke.has(e))return ke.get(e);if(n){var f=e;if(de){var o=f.v;return((f.f&C)===0&&f.reactions!==null||or(f))&&(o=Gt(f)),ke.set(f,o),o}var l=(f.f&B)===0&&!Q&&m!==null&&(ut||(m.f&B)!==0),u=(f.f&Oe)===0;it(f)&&(l&&(f.f|=B),jn(f)),l&&!u&&(zn(f),lr(f))}if(J?.has(e))return J.get(e);if((e.f&we)!==0)throw e.v;return e.v}function lr(e){if(e.f|=B,e.deps!==null)for(const t of e.deps)(t.reactions??=[]).push(e),(t.f&x)!==0&&(t.f&B)===0&&(zn(t),lr(t))}function or(e){if(e.v===P)return!0;if(e.deps===null)return!1;for(const t of e.deps)if(ke.has(t)||(t.f&x)!==0&&or(t))return!0;return!1}function Ee(e){var t=Q;try{return Q=!0,e()}finally{Q=t}}function $e(e){if(!(typeof e!="object"||!e||e instanceof EventTarget)){if(se in e)Ot(e);else if(!Array.isArray(e))for(let t in e){const n=e[t];typeof n=="object"&&n&&se in n&&Ot(n)}}}function Ot(e,t=new Set){if(typeof e=="object"&&e!==null&&!(e instanceof EventTarget)&&!t.has(e)){t.add(e),e instanceof Date&&e.getTime();for(let r in e)try{Ot(e[r],t)}catch{}const n=Ft(e);if(n!==Object.prototype&&n!==Array.prototype&&n!==Map.prototype&&n!==Set.prototype&&n!==Date.prototype){const r=wn(n);for(let i in r){const s=r[i].get;if(s)try{s.call(e)}catch{}}}}}function Oi(e){return e.endsWith("capture")&&e!=="gotpointercapture"&&e!=="lostpointercapture"}const xi=["beforeinput","click","change","dblclick","contextmenu","focusin","focusout","input","keydown","keyup","mousedown","mousemove","mouseout","mouseover","mouseup","pointerdown","pointermove","pointerout","pointerover","pointerup","touchend","touchmove","touchstart"];function Ri(e){return xi.includes(e)}const Ii={formnovalidate:"formNoValidate",ismap:"isMap",nomodule:"noModule",playsinline:"playsInline",readonly:"readOnly",defaultvalue:"defaultValue",defaultchecked:"defaultChecked",srcobject:"srcObject",novalidate:"noValidate",allowfullscreen:"allowFullscreen",disablepictureinpicture:"disablePictureInPicture",disableremoteplayback:"disableRemotePlayback"};function Li(e){return e=e.toLowerCase(),Ii[e]??e}const Di=["touchstart","touchmove"];function Fi(e){return Di.includes(e)}const Xe=Symbol("events"),ur=new Set,xt=new Set;function ar(e,t,n,r={}){function i(s){if(r.capture||Rt.call(t,s),!s.cancelBubble)return Yt(()=>n?.call(this,s))}return e.startsWith("pointer")||e.startsWith("touch")||e==="wheel"?ae(()=>{t.addEventListener(e,i,r)}):t.addEventListener(e,i,r),i}function $s(e,t,n,r,i){var s={capture:r,passive:i},f=ar(e,t,n,s);(t===document.body||t===window||t===document||t instanceof HTMLMediaElement)&&_t(()=>{t.removeEventListener(e,f,s)})}function ji(e,t,n){(t[Xe]??={})[e]=n}function zi(e){for(var t=0;t<e.length;t++)ur.add(e[t]);for(var n of xt)n(e)}let on=null;function Rt(e){var t=this,n=t.ownerDocument,r=e.type,i=e.composedPath?.()||[],s=i[0]||e.target;on=e;var f=0,o=on===e&&e[Xe];if(o){var l=i.indexOf(o);if(l!==-1&&(t===document||t===window)){e[Xe]=t;return}var u=i.indexOf(t);if(u===-1)return;l<=u&&(f=l)}if(s=i[f]||e.target,s!==t){yr(e,"currentTarget",{configurable:!0,get(){return s||n}});var a=m,c=y;Y(null),q(null);try{for(var d,v=[];s!==null;){var p=s.assignedSlot||s.parentNode||s.host||null;try{var w=s[Xe]?.[r];w!=null&&(!s.disabled||e.target===s)&&w.call(s,e)}catch(h){d?v.push(h):d=h}if(e.cancelBubble||p===t||p===null)break;s=p}if(d){for(let h of v)queueMicrotask(()=>{throw h});throw d}}finally{e[Xe]=t,delete e.currentTarget,Y(a),q(c)}}}const Hi=globalThis?.window?.trustedTypes&&globalThis.window.trustedTypes.createPolicy("svelte-trusted-html",{createHTML:e=>e});function Vi(e){return Hi?.createHTML(e)??e}function cr(e){var t=qn("template");return t.innerHTML=Vi(e.replaceAll("<!>","<!---->")),t.content}function et(e,t){var n=y;n.nodes===null&&(n.nodes={start:e,end:t,a:null,t:null})}function Ss(e,t){var n=(t&qr)!==0,r=(t&Wr)!==0,i,s=!e.startsWith("<!>");return()=>{i===void 0&&(i=cr(s?e:"<!>"+e),n||(i=Ve(i)));var f=r||Bn?document.importNode(i,!0):i.cloneNode(!0);if(n){var o=Ve(f),l=f.lastChild;et(o,l)}else et(f,f);return f}}function Bi(e,t,n="svg"){var r=!e.startsWith("<!>"),i=`<${n}>${r?e:"<!>"+e}</${n}>`,s;return()=>{if(!s){var f=cr(i),o=Ve(f);s=Ve(o)}var l=s.cloneNode(!0);return et(l,l),l}}function Ui(e,t){return Bi(e,t,"svg")}function K(){var e=document.createDocumentFragment(),t=document.createComment(""),n=ce();return e.append(t,n),et(t,n),e}function z(e,t){e!==null&&e.before(t)}function As(e,t){var n=t==null?"":typeof t=="object"?`${t}`:t;n!==(e[Nt]??=e.nodeValue)&&(e[Nt]=n,e.nodeValue=`${n}`)}function Ts(e,t){return Gi(e,t)}const lt=new Map;function Gi(e,{target:t,anchor:n,props:r={},events:i,context:s,intro:f=!0,transformError:o}){bi();var l=void 0,u=Ti(()=>{var a=n??t.appendChild(ce());ui(a,{pending:()=>{}},v=>{Pn({});var p=T;s&&(p.c=s),i&&(r.$$events=i),l=e(v,r)||{},Cn()},o);var c=new Set,d=v=>{for(var p=0;p<v.length;p++){var w=v[p];if(!c.has(w)){c.add(w);var h=Fi(w);for(const b of[t,document]){var _=lt.get(b);_===void 0&&(_=new Map,lt.set(b,_));var A=_.get(w);A===void 0?(b.addEventListener(w,Rt,{passive:h}),_.set(w,1)):_.set(w,A+1)}}}};return d(vt(ur)),xt.add(d),()=>{for(var v of c)for(const h of[t,document]){var p=lt.get(h),w=p.get(v);--w==0?(h.removeEventListener(v,Rt),p.delete(v),p.size===0&&lt.delete(h)):p.set(v,w)}xt.delete(d),a!==n&&a.parentNode?.removeChild(a)}});return Yi.set(l,u),l}let Yi=new WeakMap;class dr{anchor;#t=new Map;#l=new Map;#e=new Map;#o=new Set;#n=!0;constructor(t,n=!0){this.anchor=t,this.#n=n}#i=t=>{if(this.#t.has(t)){var n=this.#t.get(t),r=this.#l.get(n);if(r)Xt(r),this.#o.delete(n);else{var i=this.#e.get(n);i&&(this.#l.set(n,i.effect),this.#e.delete(n),i.fragment.lastChild.remove(),this.anchor.before(i.fragment),r=i.effect)}for(const[s,f]of this.#t){if(this.#t.delete(s),s===t)break;const o=this.#e.get(f);o&&(O(o.effect),this.#e.delete(f))}for(const[s,f]of this.#l){if(s===n||this.#o.has(s))continue;const o=()=>{if(Array.from(this.#t.values()).includes(s)){var u=document.createDocumentFragment();Zt(f,u),u.append(ce()),this.#e.set(s,{effect:f,fragment:u})}else O(f);this.#o.delete(s),this.#l.delete(s)};this.#n||!r?(this.#o.add(s),Ne(f,o,!1)):o()}}};#r=t=>{this.#t.delete(t);const n=Array.from(this.#t.values());for(const[r,i]of this.#e)n.includes(r)||(O(i.effect),this.#e.delete(r))};ensure(t,n){var r=E,i=Yn();if(n&&!this.#l.has(t)&&!this.#e.has(t))if(i){var s=document.createDocumentFragment(),f=ce();s.append(f),this.#e.set(t,{effect:F(()=>n(f)),fragment:s})}else this.#l.set(t,F(()=>n(this.anchor)));if(this.#t.set(r,t),i){for(const[o,l]of this.#l)o===t?r.unskip_effect(l):r.skip_effect(l);for(const[o,l]of this.#e)o===t?r.unskip_effect(l.effect):r.skip_effect(l.effect);r.oncommit(this.#i),r.ondiscard(this.#r)}else this.#i(r)}}function ks(e,t,n=!1){var r=new dr(e),i=n?Ce:0;function s(f,o){r.ensure(f,o)}gt(()=>{var f=!1;t((o,l=0)=>{f=!0,s(l,o)}),f||s(-1,null)},i)}function qi(e,t){return t}function Wi(e,t,n){for(var r=[],i=t.length,s,f=t.length,o=0;o<i;o++){let c=t[o];Ne(c,()=>{if(s){if(s.pending.delete(c),s.done.add(c),s.pending.size===0){var d=e.outrogroups;It(e,vt(s.done)),d.delete(s),d.size===0&&(e.outrogroups=null)}}else f-=1},!1)}if(f===0){var l=r.length===0&&n!==null;if(l){var u=n,a=u.parentNode;Ei(a),a.append(u),e.items.clear()}It(e,t,!l)}else s={pending:new Set(t),done:new Set},(e.outrogroups??=new Set).add(s)}function It(e,t,n=!0){var r;if(e.pending.size>0){r=new Set;for(const f of e.pending.values())for(const o of f)r.add(e.items.get(o).e)}for(var i=0;i<t.length;i++){var s=t[i];if(r?.has(s)){s.f|=ie;const f=document.createDocumentFragment();Zt(s,f)}else O(t[i],n)}}var un;function Ki(e,t,n,r,i,s=null){var f=e,o=new Map,l=(t&Sn)!==0;if(l){var u=e;f=u.appendChild(ce())}var a=null,c=Ut(()=>{var b=n();return Dt(b)?b:b==null?[]:vt(b)}),d,v=new Map,p=!0;function w(b){(A.effect.f&U)===0&&(A.pending.delete(b),A.fallback=a,Xi(A,d,f,t,r),a!==null&&(d.length===0?(a.f&ie)===0?Xt(a):(a.f^=ie,Ze(a,null,f)):Ne(a,()=>{a=null})))}function h(b){A.pending.delete(b)}var _=gt(()=>{d=N(c);for(var b=d.length,g=new Set,$=E,R=Yn(),S=0;S<b;S+=1){var le=d[S],he=r(le,S),I=p?null:o.get(he);I?(I.v&&He(I.v,le),I.i&&He(I.i,S),R&&$.unskip_effect(I.e)):(I=Zi(o,p?f:un??=ce(),le,he,S,i,t,n),p||(I.e.f|=ie),o.set(he,I)),g.add(he)}if(b===0&&s&&!a&&(p?a=F(()=>s(f)):(a=F(()=>s(un??=ce())),a.f|=ie)),b>g.size&&Pr(),!p)if(v.set($,g),R){for(const[gr,wr]of o)g.has(gr)||$.skip_effect(wr.e);$.oncommit(w),$.ondiscard(h)}else w($);N(c)}),A={effect:_,items:o,pending:v,outrogroups:null,fallback:a};p=!1}function qe(e){for(;e!==null&&(e.f&ee)===0;)e=e.next;return e}function Xi(e,t,n,r,i){var s=(r&Hr)!==0,f=t.length,o=e.items,l=qe(e.effect.first),u,a=null,c,d=[],v=[],p,w,h,_;if(s)for(_=0;_<f;_+=1)p=t[_],w=i(p,_),h=o.get(w).e,(h.f&ie)===0&&(h.nodes?.a?.measure(),(c??=new Set).add(h));for(_=0;_<f;_+=1){if(p=t[_],w=i(p,_),h=o.get(w).e,e.outrogroups!==null)for(const I of e.outrogroups)I.pending.delete(h),I.done.delete(h);if((h.f&L)!==0&&(Xt(h),s&&(h.nodes?.a?.unfix(),(c??=new Set).delete(h))),(h.f&ie)!==0)if(h.f^=ie,h===l)Ze(h,null,n);else{var A=a?a.next:l;h===e.effect.last&&(e.effect.last=h.prev),h.prev&&(h.prev.next=h.next),h.next&&(h.next.prev=h.prev),pe(e,a,h),pe(e,h,A),Ze(h,A,n),a=h,d=[],v=[],l=qe(a.next);continue}if(h!==l){if(u!==void 0&&u.has(h)){if(d.length<v.length){var b=v[0],g;a=b.prev;var $=d[0],R=d[d.length-1];for(g=0;g<d.length;g+=1)Ze(d[g],b,n);for(g=0;g<v.length;g+=1)u.delete(v[g]);pe(e,$.prev,R.next),pe(e,a,$),pe(e,R,b),l=b,a=R,_-=1,d=[],v=[]}else u.delete(h),Ze(h,l,n),pe(e,h.prev,h.next),pe(e,h,a===null?e.effect.first:a.next),pe(e,a,h),a=h;continue}for(d=[],v=[];l!==null&&l!==h;)(u??=new Set).add(l),v.push(l),l=qe(l.next);if(l===null)continue}(h.f&ie)===0&&d.push(h),a=h,l=qe(h.next)}if(e.outrogroups!==null){for(const I of e.outrogroups)I.pending.size===0&&(It(e,vt(I.done)),e.outrogroups?.delete(I));e.outrogroups.size===0&&(e.outrogroups=null)}if(l!==null||u!==void 0){var S=[];if(u!==void 0)for(h of u)(h.f&L)===0&&S.push(h);for(;l!==null;)(l.f&L)===0&&l!==e.fallback&&S.push(l),l=qe(l.next);var le=S.length;if(le>0){var he=(r&Sn)!==0&&f===0?n:null;if(s){for(_=0;_<le;_+=1)S[_].nodes?.a?.measure();for(_=0;_<le;_+=1)S[_].nodes?.a?.fix()}Wi(e,S,he)}}s&&ae(()=>{if(c!==void 0)for(h of c)h.nodes?.a?.apply()})}function Zi(e,t,n,r,i,s,f,o){var l=(f&jr)!==0?(f&Vr)===0?_i(n,!1,!1):me(n):null,u=(f&zr)!==0?me(i):null;return{v:l,i:u,e:F(()=>(s(t,l??n,u??i,o),()=>{e.delete(r)}))}}function Ze(e,t,n){if(e.nodes)for(var r=e.nodes.start,i=e.nodes.end,s=t&&(t.f&ie)===0?t.nodes.start:n;r!==null;){var f=rt(r);if(s.before(r),r===i)return;r=f}}function pe(e,t,n){t===null?e.effect.first=n:t.next=n,n===null?e.effect.last=t:n.prev=t}function X(e,t,n,r,i){var s=t.$$slots?.[n],f=!1;s===!0&&(s=t.children,f=!0),s===void 0||s(e,f?()=>r:r)}function Ji(e,t,n,r,i,s){var f=null,o=e,l=new dr(o,!1);gt(()=>{const u=t()||null;var a=Kr;if(u===null){l.ensure(null,null);return}return l.ensure(u,c=>{if(u){if(f=qn(u,a),et(f,f),r){var d=f.appendChild(ce());r(f,d)}y.nodes.end=f,c.before(f)}}),()=>{}},Ce),_t(()=>{})}function Qi(e,t){var n=void 0,r;Zn(()=>{n!==(n=t())&&(r&&(O(r),r=null),n&&(r=F(()=>{Wt(()=>n(e))})))})}function hr(e){var t,n,r="";if(typeof e=="string"||typeof e=="number")r+=e;else if(typeof e=="object")if(Array.isArray(e)){var i=e.length;for(t=0;t<i;t++)e[t]&&(n=hr(e[t]))&&(r&&(r+=" "),r+=n)}else for(n in e)e[n]&&(r&&(r+=" "),r+=n);return r}function es(){for(var e,t,n=0,r="",i=arguments.length;n<i;n++)(e=arguments[n])&&(t=hr(e))&&(r&&(r+=" "),r+=t);return r}function ts(e){return typeof e=="object"?es(e):e??""}const an=[...` 	
\r\f \v\uFEFF`];function ns(e,t,n){var r=e==null?"":""+e;if(t&&(r=r?r+" "+t:t),n){for(var i of Object.keys(n))if(n[i])r=r?r+" "+i:i;else if(r.length)for(var s=i.length,f=0;(f=r.indexOf(i,f))>=0;){var o=f+s;(f===0||an.includes(r[f-1]))&&(o===r.length||an.includes(r[o]))?r=(f===0?"":r.substring(0,f))+r.substring(o+1):f=o}}return r===""?null:r}function cn(e,t=!1){var n=t?" !important;":";",r="";for(var i of Object.keys(e)){var s=e[i];s!=null&&s!==""&&(r+=" "+i+": "+s+n)}return r}function yt(e){return e[0]!=="-"||e[1]!=="-"?e.toLowerCase():e}function rs(e,t){if(t){var n="",r,i;if(Array.isArray(t)?(r=t[0],i=t[1]):r=t,e){e=String(e).replaceAll(/\s*\/\*.*?\*\/\s*/g,"").trim();var s=!1,f=0,o=!1,l=[];r&&l.push(...Object.keys(r).map(yt)),i&&l.push(...Object.keys(i).map(yt));var u=0,a=-1;const w=e.length;for(var c=0;c<w;c++){var d=e[c];if(o?d==="/"&&e[c-1]==="*"&&(o=!1):s?s===d&&(s=!1):d==="/"&&e[c+1]==="*"?o=!0:d==='"'||d==="'"?s=d:d==="("?f++:d===")"&&f--,!o&&s===!1&&f===0){if(d===":"&&a===-1)a=c;else if(d===";"||c===w-1){if(a!==-1){var v=yt(e.substring(u,a).trim());if(!l.includes(v)){d!==";"&&c++;var p=e.substring(u,c).trim();n+=" "+p+";"}}u=c+1,a=-1}}}}return r&&(n+=cn(r)),i&&(n+=cn(i,!0)),n=n.trim(),n===""?null:n}return e==null?null:String(e)}function is(e,t,n,r,i,s){var f=e[Tt];if(f!==n||f===void 0){var o=ns(n,r,s);o==null?e.removeAttribute("class"):t?e.className=o:e.setAttribute("class",o),e[Tt]=n}else if(s&&i!==s)for(var l in s){var u=!!s[l];(i==null||u!==!!i[l])&&e.classList.toggle(l,u)}return s}function mt(e,t={},n,r){for(var i in n){var s=n[i];t[i]!==s&&(n[i]==null?e.style.removeProperty(i):e.style.setProperty(i,s,r))}}function ss(e,t,n,r){var i=e[kt];if(i!==t){var s=rs(t,r);s==null?e.removeAttribute("style"):e.style.cssText=s,e[kt]=t}else r&&(Array.isArray(r)?(mt(e,n?.[0],r[0]),mt(e,n?.[1],r[1],"important")):mt(e,n,r));return r}function Lt(e,t,n=!1){if(e.multiple){if(t==null)return;if(!Dt(t))return Jr();for(var r of e.options)r.selected=t.includes(dn(r));return}for(r of e.options){var i=dn(r);if(wi(i,t)){r.selected=!0;return}}(!n||t!==void 0)&&(e.selectedIndex=-1)}function fs(e){var t=new MutationObserver(()=>{Lt(e,e.__value)});t.observe(e,{childList:!0,subtree:!0,attributes:!0,attributeFilter:["value"]}),_t(()=>{t.disconnect()})}function dn(e){return"__value"in e?e.__value:e.value}const We=Symbol("class"),Ke=Symbol("style"),vr=Symbol("is custom element"),pr=Symbol("is html"),ls=zt?"option":"OPTION",os=zt?"select":"SELECT",us=zt?"progress":"PROGRESS";function Ns(e,t){var n=Jt(e);n.value===(n.value=t??void 0)||e.value===t&&(t!==0||e.nodeName!==us)||(e.value=t??"")}function as(e,t){t?e.hasAttribute("selected")||e.setAttribute("selected",""):e.removeAttribute("selected")}function hn(e,t,n,r){var i=Jt(e);i[t]!==(i[t]=n)&&(t==="loading"&&(e[kr]=n),n==null?e.removeAttribute(t):typeof n!="string"&&_r(e).includes(t)?e[t]=n:e.setAttribute(t,n))}function cs(e,t,n,r,i=!1,s=!1){var f=Jt(e),o=f[vr],l=!f[pr],u=t||{},a=e.nodeName===ls;for(var c in t)c in n||(n[c]=null);n.class?n.class=ts(n.class):n[We]&&(n.class=null),n[Ke]&&(n.style??=null);var d=_r(e);for(const b in n){let g=n[b];if(a&&b==="value"&&g==null){e.value=e.__value="",u[b]=g;continue}if(b==="class"){var v=e.namespaceURI==="http://www.w3.org/1999/xhtml";is(e,v,g,r,t?.[We],n[We]),u[b]=g,u[We]=n[We];continue}if(b==="style"){ss(e,g,t?.[Ke],n[Ke]),u[b]=g,u[Ke]=n[Ke];continue}var p=u[b];if(!(g===p&&!(g===void 0&&e.hasAttribute(b)))){u[b]=g;var w=b[0]+b[1];if(w!=="$$")if(w==="on"){const $={},R="$$"+b;let S=b.slice(2);var h=Ri(S);if(Oi(S)&&(S=S.slice(0,-7),$.capture=!0),!h&&p){if(g!=null)continue;e.removeEventListener(S,u[R],$),u[R]=null}if(h)ji(S,e,g),zi([S]);else if(g!=null){let le=function(he){u[b].call(this,he)};u[R]=ar(S,e,le,$)}}else if(b==="style")hn(e,b,g);else if(b==="autofocus")$i(e,!!g);else if(!o&&(b==="__value"||b==="value"&&g!=null))e.value=e.__value=g;else if(b==="selected"&&a)as(e,g);else{var _=b;l||(_=Li(_));var A=_==="defaultValue"||_==="defaultChecked";if(g==null&&!o&&!A)if(f[b]=null,_==="value"||_==="checked"){let $=e;const R=t===void 0;if(_==="value"){let S=$.defaultValue;$.removeAttribute(_),$.defaultValue=S,$.value=$.__value=R?S:null}else{let S=$.defaultChecked;$.removeAttribute(_),$.defaultChecked=S,$.checked=R?S:!1}}else e.removeAttribute(b);else A||d.includes(_)&&(o||typeof g!="string")?(e[_]=g,_ in f&&(f[_]=P)):typeof g!="function"&&hn(e,_,g)}}}return u}function vn(e,t,n=[],r=[],i=[],s,f=!1,o=!1){Dn(i,n,r,l=>{var u=void 0,a={},c=e.nodeName===os,d=!1;if(Zn(()=>{var p=t(...l.map(N)),w=cs(e,u,p,s,f,o);d&&c&&"value"in p&&Lt(e,p.value);for(let _ of Object.getOwnPropertySymbols(a))p[_]||O(a[_]);for(let _ of Object.getOwnPropertySymbols(p)){var h=p[_];_.description===Xr&&(!u||h!==u[_])&&(a[_]&&O(a[_]),a[_]=F(()=>Qi(e,()=>h))),w[_]=h}u=w}),c){var v=e;Wt(()=>{Lt(v,u.value,!0),fs(v)})}d=!0})}function Jt(e){return e[En]??={[vr]:e.nodeName.includes("-"),[pr]:e.namespaceURI===Tn}}var pn=new Map;function _r(e){var t=e.getAttribute("is")||e.nodeName,n=pn.get(t);if(n)return n;pn.set(t,n=[]);for(var r,i=e,s=Element.prototype;s!==i;){r=wn(i);for(var f in r)r[f].set&&f!=="innerHTML"&&f!=="textContent"&&f!=="innerText"&&n.push(f);i=Ft(i)}return n}function Et(e,t){return e===t||e?.[se]===t}function Ps(e={},t,n,r){var i=T.r,s=y;return Wt(()=>{var f,o;return Xn(()=>{f=o,o=[],Ee(()=>{Et(n(...o),e)||(t(e,...o),f&&Et(n(...f),e)&&t(null,...f))})}),()=>{let l=s;for(;l!==i&&l.parent!==null&&l.parent.f&At;)l=l.parent;const u=()=>{o&&Et(n(...o),e)&&t(null,...o)},a=l.teardown;l.teardown=()=>{u(),a?.()}}}),e}function ds(e=!1){const t=T,n=t.l.u;if(!n)return;let r=()=>$e(t.s);if(e){let i=0,s={};const f=ze(()=>{let o=!1;const l=t.s;for(const u in l)l[u]!==s[u]&&(s[u]=l[u],o=!0);return o&&i++,i});r=()=>N(f)}n.b.length&&Ai(()=>{_n(t,r),$t(n.b)}),Mt(()=>{const i=Ee(()=>n.m.map(Sr));return()=>{for(const s of i)typeof s=="function"&&s()}}),n.a.length&&Mt(()=>{_n(t,r),$t(n.a)})}function _n(e,t){if(e.l.s)for(const n of e.l.s)N(n);t()}const hs={get(e,t){if(!e.exclude.includes(t))return N(e.version),t in e.special?e.special[t]():e.props[t]},set(e,t,n){if(!(t in e.special)){var r=y;try{q(e.parent_effect),e.special[t]=Se({get[t](){return e.props[t]}},t,An)}finally{q(r)}}return e.special[t](n),nn(e.version),!0},getOwnPropertyDescriptor(e,t){if(!e.exclude.includes(t)&&t in e.props)return{enumerable:!0,configurable:!0,value:e.props[t]}},deleteProperty(e,t){return e.exclude.includes(t)||(e.exclude.push(t),nn(e.version)),!0},has(e,t){return e.exclude.includes(t)?!1:t in e.props},ownKeys(e){return Reflect.ownKeys(e.props).filter(t=>!e.exclude.includes(t))}};function H(e,t){return new Proxy({props:e,exclude:t,special:{},version:me(0),parent_effect:y},hs)}const vs={get(e,t){let n=e.props.length;for(;n--;){let r=e.props[n];if(Ye(r)&&(r=r()),typeof r=="object"&&r!==null&&t in r)return r[t]}},set(e,t,n){let r=e.props.length;for(;r--;){let i=e.props[r];Ye(i)&&(i=i());const s=ge(i,t);if(s&&s.set)return s.set(n),!0}return!1},getOwnPropertyDescriptor(e,t){let n=e.props.length;for(;n--;){let r=e.props[n];if(Ye(r)&&(r=r()),typeof r=="object"&&r!==null&&t in r){const i=ge(r,t);return i&&!i.configurable&&(i.configurable=!0),i}}},has(e,t){if(t===se||t===mn)return!1;for(let n of e.props)if(Ye(n)&&(n=n()),n!=null&&t in n)return!0;return!1},ownKeys(e){const t=[];for(let n of e.props)if(Ye(n)&&(n=n()),!!n){for(const r in n)t.includes(r)||t.push(r);for(const r of Object.getOwnPropertySymbols(n))t.includes(r)||t.push(r)}return t}};function ne(...e){return new Proxy({props:e},vs)}function Se(e,t,n,r){var i=!Ge||(n&Ur)!==0,s=(n&Gr)!==0,f=(n&Yr)!==0,o=r,l=!0,u=void 0,a=()=>f&&i?(u??=ze(r),N(u)):(l&&(l=!1,o=f?Ee(r):r),o);let c;if(s){var d=se in e||mn in e;c=ge(e,t)?.set??(d&&t in e?g=>e[t]=g:void 0)}var v,p=!1;s?[v,p]=ii(()=>e[t]):v=e[t],v===void 0&&r!==void 0&&(v=a(),c&&(i&&Rr(),c(v)));var w;if(i?w=()=>{var g=e[t];return g===void 0?a():(l=!0,g)}:w=()=>{var g=e[t];return g!==void 0&&(o=void 0),g===void 0?o:g},i&&(n&An)===0)return w;if(c){var h=e.$$legacy;return(function(g,$){return arguments.length>0?((!i||!$||h||p)&&c($?w():g),g):w()})}var _=!1,A=((n&Br)!==0?ze:Ut)(()=>(_=!1,w()));s&&N(A);var b=y;return(function(g,$){if(arguments.length>0){const R=$?N(A):i&&s?Le(g):g;return ue(A,R),_=!0,o!==void 0&&(o=R),g}return de&&_||(b.f&U)!==0?A.v:N(A)})}function ps(e){T===null&&$n(),Ge&&T.l!==null?_s(T).m.push(e):Mt(()=>{const t=Ee(e);if(typeof t=="function")return t})}function Cs(e){T===null&&$n(),ps(()=>()=>Ee(e))}function _s(e){var t=e.l;return t.u??={a:[],b:[],m:[]}}const gs="5";typeof window<"u"&&((window.__svelte??={}).v??=new Set).add(gs);ti();/**
 * @license lucide-svelte v1.0.1 - ISC
 *
 * ISC License
 * 
 * Copyright (c) 2026 Lucide Icons and Contributors
 * 
 * Permission to use, copy, modify, and/or distribute this software for any
 * purpose with or without fee is hereby granted, provided that the above
 * copyright notice and this permission notice appear in all copies.
 * 
 * THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
 * WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
 * MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR
 * ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
 * WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
 * ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
 * OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.
 * 
 * ---
 * 
 * The following Lucide icons are derived from the Feather project:
 * 
 * airplay, alert-circle, alert-octagon, alert-triangle, aperture, arrow-down-circle, arrow-down-left, arrow-down-right, arrow-down, arrow-left-circle, arrow-left, arrow-right-circle, arrow-right, arrow-up-circle, arrow-up-left, arrow-up-right, arrow-up, at-sign, calendar, cast, check, chevron-down, chevron-left, chevron-right, chevron-up, chevrons-down, chevrons-left, chevrons-right, chevrons-up, circle, clipboard, clock, code, columns, command, compass, corner-down-left, corner-down-right, corner-left-down, corner-left-up, corner-right-down, corner-right-up, corner-up-left, corner-up-right, crosshair, database, divide-circle, divide-square, dollar-sign, download, external-link, feather, frown, hash, headphones, help-circle, info, italic, key, layout, life-buoy, link-2, link, loader, lock, log-in, log-out, maximize, meh, minimize, minimize-2, minus-circle, minus-square, minus, monitor, moon, more-horizontal, more-vertical, move, music, navigation-2, navigation, octagon, pause-circle, percent, plus-circle, plus-square, plus, power, radio, rss, search, server, share, shopping-bag, sidebar, smartphone, smile, square, table-2, tablet, target, terminal, trash-2, trash, triangle, tv, type, upload, x-circle, x-octagon, x-square, x, zoom-in, zoom-out
 * 
 * The MIT License (MIT) (for the icons listed above)
 * 
 * Copyright (c) 2013-present Cole Bemis
 * 
 * Permission is hereby granted, free of charge, to any person obtaining a copy
 * of this software and associated documentation files (the "Software"), to deal
 * in the Software without restriction, including without limitation the rights
 * to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
 * copies of the Software, and to permit persons to whom the Software is
 * furnished to do so, subject to the following conditions:
 * 
 * The above copyright notice and this permission notice shall be included in all
 * copies or substantial portions of the Software.
 * 
 * THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
 * IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
 * FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
 * AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
 * LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
 * OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
 * SOFTWARE.
 * 
 */const ws={xmlns:"http://www.w3.org/2000/svg",width:24,height:24,viewBox:"0 0 24 24",fill:"none",stroke:"currentColor","stroke-width":2,"stroke-linecap":"round","stroke-linejoin":"round"};/**
 * @license lucide-svelte v1.0.1 - ISC
 *
 * ISC License
 * 
 * Copyright (c) 2026 Lucide Icons and Contributors
 * 
 * Permission to use, copy, modify, and/or distribute this software for any
 * purpose with or without fee is hereby granted, provided that the above
 * copyright notice and this permission notice appear in all copies.
 * 
 * THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
 * WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
 * MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR
 * ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
 * WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
 * ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
 * OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.
 * 
 * ---
 * 
 * The following Lucide icons are derived from the Feather project:
 * 
 * airplay, alert-circle, alert-octagon, alert-triangle, aperture, arrow-down-circle, arrow-down-left, arrow-down-right, arrow-down, arrow-left-circle, arrow-left, arrow-right-circle, arrow-right, arrow-up-circle, arrow-up-left, arrow-up-right, arrow-up, at-sign, calendar, cast, check, chevron-down, chevron-left, chevron-right, chevron-up, chevrons-down, chevrons-left, chevrons-right, chevrons-up, circle, clipboard, clock, code, columns, command, compass, corner-down-left, corner-down-right, corner-left-down, corner-left-up, corner-right-down, corner-right-up, corner-up-left, corner-up-right, crosshair, database, divide-circle, divide-square, dollar-sign, download, external-link, feather, frown, hash, headphones, help-circle, info, italic, key, layout, life-buoy, link-2, link, loader, lock, log-in, log-out, maximize, meh, minimize, minimize-2, minus-circle, minus-square, minus, monitor, moon, more-horizontal, more-vertical, move, music, navigation-2, navigation, octagon, pause-circle, percent, plus-circle, plus-square, plus, power, radio, rss, search, server, share, shopping-bag, sidebar, smartphone, smile, square, table-2, tablet, target, terminal, trash-2, trash, triangle, tv, type, upload, x-circle, x-octagon, x-square, x, zoom-in, zoom-out
 * 
 * The MIT License (MIT) (for the icons listed above)
 * 
 * Copyright (c) 2013-present Cole Bemis
 * 
 * Permission is hereby granted, free of charge, to any person obtaining a copy
 * of this software and associated documentation files (the "Software"), to deal
 * in the Software without restriction, including without limitation the rights
 * to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
 * copies of the Software, and to permit persons to whom the Software is
 * furnished to do so, subject to the following conditions:
 * 
 * The above copyright notice and this permission notice shall be included in all
 * copies or substantial portions of the Software.
 * 
 * THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
 * IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
 * FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
 * AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
 * LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
 * OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
 * SOFTWARE.
 * 
 */const bs=e=>{for(const t in e)if(t.startsWith("aria-")||t==="role"||t==="title")return!0;return!1};/**
 * @license lucide-svelte v1.0.1 - ISC
 *
 * ISC License
 * 
 * Copyright (c) 2026 Lucide Icons and Contributors
 * 
 * Permission to use, copy, modify, and/or distribute this software for any
 * purpose with or without fee is hereby granted, provided that the above
 * copyright notice and this permission notice appear in all copies.
 * 
 * THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
 * WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
 * MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR
 * ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
 * WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
 * ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
 * OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.
 * 
 * ---
 * 
 * The following Lucide icons are derived from the Feather project:
 * 
 * airplay, alert-circle, alert-octagon, alert-triangle, aperture, arrow-down-circle, arrow-down-left, arrow-down-right, arrow-down, arrow-left-circle, arrow-left, arrow-right-circle, arrow-right, arrow-up-circle, arrow-up-left, arrow-up-right, arrow-up, at-sign, calendar, cast, check, chevron-down, chevron-left, chevron-right, chevron-up, chevrons-down, chevrons-left, chevrons-right, chevrons-up, circle, clipboard, clock, code, columns, command, compass, corner-down-left, corner-down-right, corner-left-down, corner-left-up, corner-right-down, corner-right-up, corner-up-left, corner-up-right, crosshair, database, divide-circle, divide-square, dollar-sign, download, external-link, feather, frown, hash, headphones, help-circle, info, italic, key, layout, life-buoy, link-2, link, loader, lock, log-in, log-out, maximize, meh, minimize, minimize-2, minus-circle, minus-square, minus, monitor, moon, more-horizontal, more-vertical, move, music, navigation-2, navigation, octagon, pause-circle, percent, plus-circle, plus-square, plus, power, radio, rss, search, server, share, shopping-bag, sidebar, smartphone, smile, square, table-2, tablet, target, terminal, trash-2, trash, triangle, tv, type, upload, x-circle, x-octagon, x-square, x, zoom-in, zoom-out
 * 
 * The MIT License (MIT) (for the icons listed above)
 * 
 * Copyright (c) 2013-present Cole Bemis
 * 
 * Permission is hereby granted, free of charge, to any person obtaining a copy
 * of this software and associated documentation files (the "Software"), to deal
 * in the Software without restriction, including without limitation the rights
 * to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
 * copies of the Software, and to permit persons to whom the Software is
 * furnished to do so, subject to the following conditions:
 * 
 * The above copyright notice and this permission notice shall be included in all
 * copies or substantial portions of the Software.
 * 
 * THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
 * IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
 * FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
 * AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
 * LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
 * OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
 * SOFTWARE.
 * 
 */const gn=(...e)=>e.filter((t,n,r)=>!!t&&t.trim()!==""&&r.indexOf(t)===n).join(" ").trim();var ys=Ui("<svg><!><!></svg>");function re(e,t){const n=H(t,["children","$$slots","$$events","$$legacy"]),r=H(n,["name","color","size","strokeWidth","absoluteStrokeWidth","iconNode"]);Pn(t,!1);let i=Se(t,"name",8,void 0),s=Se(t,"color",8,"currentColor"),f=Se(t,"size",8,24),o=Se(t,"strokeWidth",8,2),l=Se(t,"absoluteStrokeWidth",8,!1),u=Se(t,"iconNode",24,()=>[]);ds();var a=ys();vn(a,(v,p,w)=>({...ws,...v,...r,width:f(),height:f(),stroke:s(),"stroke-width":p,class:w}),[()=>bs(r)?void 0:{"aria-hidden":"true"},()=>($e(l()),$e(o()),$e(f()),Ee(()=>l()?Number(o())*24/Number(f()):o())),()=>($e(gn),$e(i()),$e(n),Ee(()=>gn("lucide-icon","lucide",i()?`lucide-${i()}`:"",n.class)))]);var c=yi(a);Ki(c,1,u,qi,(v,p)=>{var w=hi(()=>Ar(N(p),2));let h=()=>N(w)[0],_=()=>N(w)[1];var A=K(),b=W(A);Ji(b,h,!0,(g,$)=>{vn(g,()=>({..._()}))}),z(v,A)});var d=mi(c);X(d,t,"default",{}),z(e,a),Cn()}function Ms(e,t){const n=H(t,["children","$$slots","$$events","$$legacy"]);/**
 * @license lucide-svelte v1.0.1 - ISC
 *
 * ISC License
 *
 * Copyright (c) 2026 Lucide Icons and Contributors
 *
 * Permission to use, copy, modify, and/or distribute this software for any
 * purpose with or without fee is hereby granted, provided that the above
 * copyright notice and this permission notice appear in all copies.
 *
 * THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
 * WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
 * MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR
 * ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
 * WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
 * ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
 * OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.
 *
 * ---
 *
 * The following Lucide icons are derived from the Feather project:
 *
 * airplay, alert-circle, alert-octagon, alert-triangle, aperture, arrow-down-circle, arrow-down-left, arrow-down-right, arrow-down, arrow-left-circle, arrow-left, arrow-right-circle, arrow-right, arrow-up-circle, arrow-up-left, arrow-up-right, arrow-up, at-sign, calendar, cast, check, chevron-down, chevron-left, chevron-right, chevron-up, chevrons-down, chevrons-left, chevrons-right, chevrons-up, circle, clipboard, clock, code, columns, command, compass, corner-down-left, corner-down-right, corner-left-down, corner-left-up, corner-right-down, corner-right-up, corner-up-left, corner-up-right, crosshair, database, divide-circle, divide-square, dollar-sign, download, external-link, feather, frown, hash, headphones, help-circle, info, italic, key, layout, life-buoy, link-2, link, loader, lock, log-in, log-out, maximize, meh, minimize, minimize-2, minus-circle, minus-square, minus, monitor, moon, more-horizontal, more-vertical, move, music, navigation-2, navigation, octagon, pause-circle, percent, plus-circle, plus-square, plus, power, radio, rss, search, server, share, shopping-bag, sidebar, smartphone, smile, square, table-2, tablet, target, terminal, trash-2, trash, triangle, tv, type, upload, x-circle, x-octagon, x-square, x, zoom-in, zoom-out
 *
 * The MIT License (MIT) (for the icons listed above)
 *
 * Copyright (c) 2013-present Cole Bemis
 *
 * Permission is hereby granted, free of charge, to any person obtaining a copy
 * of this software and associated documentation files (the "Software"), to deal
 * in the Software without restriction, including without limitation the rights
 * to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
 * copies of the Software, and to permit persons to whom the Software is
 * furnished to do so, subject to the following conditions:
 *
 * The above copyright notice and this permission notice shall be included in all
 * copies or substantial portions of the Software.
 *
 * THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
 * IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
 * FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
 * AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
 * LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
 * OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
 * SOFTWARE.
 *
 */const r=[["path",{d:"m5 12 7-7 7 7"}],["path",{d:"M12 19V5"}]];re(e,ne({name:"arrow-up"},()=>n,{get iconNode(){return r},children:(i,s)=>{var f=K(),o=W(f);X(o,t,"default",{}),z(i,f)},$$slots:{default:!0}}))}function Os(e,t){const n=H(t,["children","$$slots","$$events","$$legacy"]);/**
 * @license lucide-svelte v1.0.1 - ISC
 *
 * ISC License
 *
 * Copyright (c) 2026 Lucide Icons and Contributors
 *
 * Permission to use, copy, modify, and/or distribute this software for any
 * purpose with or without fee is hereby granted, provided that the above
 * copyright notice and this permission notice appear in all copies.
 *
 * THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
 * WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
 * MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR
 * ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
 * WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
 * ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
 * OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.
 *
 * ---
 *
 * The following Lucide icons are derived from the Feather project:
 *
 * airplay, alert-circle, alert-octagon, alert-triangle, aperture, arrow-down-circle, arrow-down-left, arrow-down-right, arrow-down, arrow-left-circle, arrow-left, arrow-right-circle, arrow-right, arrow-up-circle, arrow-up-left, arrow-up-right, arrow-up, at-sign, calendar, cast, check, chevron-down, chevron-left, chevron-right, chevron-up, chevrons-down, chevrons-left, chevrons-right, chevrons-up, circle, clipboard, clock, code, columns, command, compass, corner-down-left, corner-down-right, corner-left-down, corner-left-up, corner-right-down, corner-right-up, corner-up-left, corner-up-right, crosshair, database, divide-circle, divide-square, dollar-sign, download, external-link, feather, frown, hash, headphones, help-circle, info, italic, key, layout, life-buoy, link-2, link, loader, lock, log-in, log-out, maximize, meh, minimize, minimize-2, minus-circle, minus-square, minus, monitor, moon, more-horizontal, more-vertical, move, music, navigation-2, navigation, octagon, pause-circle, percent, plus-circle, plus-square, plus, power, radio, rss, search, server, share, shopping-bag, sidebar, smartphone, smile, square, table-2, tablet, target, terminal, trash-2, trash, triangle, tv, type, upload, x-circle, x-octagon, x-square, x, zoom-in, zoom-out
 *
 * The MIT License (MIT) (for the icons listed above)
 *
 * Copyright (c) 2013-present Cole Bemis
 *
 * Permission is hereby granted, free of charge, to any person obtaining a copy
 * of this software and associated documentation files (the "Software"), to deal
 * in the Software without restriction, including without limitation the rights
 * to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
 * copies of the Software, and to permit persons to whom the Software is
 * furnished to do so, subject to the following conditions:
 *
 * The above copyright notice and this permission notice shall be included in all
 * copies or substantial portions of the Software.
 *
 * THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
 * IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
 * FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
 * AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
 * LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
 * OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
 * SOFTWARE.
 *
 */const r=[["path",{d:"m6 9 6 6 6-6"}]];re(e,ne({name:"chevron-down"},()=>n,{get iconNode(){return r},children:(i,s)=>{var f=K(),o=W(f);X(o,t,"default",{}),z(i,f)},$$slots:{default:!0}}))}function xs(e,t){const n=H(t,["children","$$slots","$$events","$$legacy"]);/**
 * @license lucide-svelte v1.0.1 - ISC
 *
 * ISC License
 *
 * Copyright (c) 2026 Lucide Icons and Contributors
 *
 * Permission to use, copy, modify, and/or distribute this software for any
 * purpose with or without fee is hereby granted, provided that the above
 * copyright notice and this permission notice appear in all copies.
 *
 * THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
 * WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
 * MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR
 * ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
 * WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
 * ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
 * OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.
 *
 * ---
 *
 * The following Lucide icons are derived from the Feather project:
 *
 * airplay, alert-circle, alert-octagon, alert-triangle, aperture, arrow-down-circle, arrow-down-left, arrow-down-right, arrow-down, arrow-left-circle, arrow-left, arrow-right-circle, arrow-right, arrow-up-circle, arrow-up-left, arrow-up-right, arrow-up, at-sign, calendar, cast, check, chevron-down, chevron-left, chevron-right, chevron-up, chevrons-down, chevrons-left, chevrons-right, chevrons-up, circle, clipboard, clock, code, columns, command, compass, corner-down-left, corner-down-right, corner-left-down, corner-left-up, corner-right-down, corner-right-up, corner-up-left, corner-up-right, crosshair, database, divide-circle, divide-square, dollar-sign, download, external-link, feather, frown, hash, headphones, help-circle, info, italic, key, layout, life-buoy, link-2, link, loader, lock, log-in, log-out, maximize, meh, minimize, minimize-2, minus-circle, minus-square, minus, monitor, moon, more-horizontal, more-vertical, move, music, navigation-2, navigation, octagon, pause-circle, percent, plus-circle, plus-square, plus, power, radio, rss, search, server, share, shopping-bag, sidebar, smartphone, smile, square, table-2, tablet, target, terminal, trash-2, trash, triangle, tv, type, upload, x-circle, x-octagon, x-square, x, zoom-in, zoom-out
 *
 * The MIT License (MIT) (for the icons listed above)
 *
 * Copyright (c) 2013-present Cole Bemis
 *
 * Permission is hereby granted, free of charge, to any person obtaining a copy
 * of this software and associated documentation files (the "Software"), to deal
 * in the Software without restriction, including without limitation the rights
 * to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
 * copies of the Software, and to permit persons to whom the Software is
 * furnished to do so, subject to the following conditions:
 *
 * The above copyright notice and this permission notice shall be included in all
 * copies or substantial portions of the Software.
 *
 * THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
 * IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
 * FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
 * AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
 * LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
 * OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
 * SOFTWARE.
 *
 */const r=[["path",{d:"m9 18 6-6-6-6"}]];re(e,ne({name:"chevron-right"},()=>n,{get iconNode(){return r},children:(i,s)=>{var f=K(),o=W(f);X(o,t,"default",{}),z(i,f)},$$slots:{default:!0}}))}function Rs(e,t){const n=H(t,["children","$$slots","$$events","$$legacy"]);/**
 * @license lucide-svelte v1.0.1 - ISC
 *
 * ISC License
 *
 * Copyright (c) 2026 Lucide Icons and Contributors
 *
 * Permission to use, copy, modify, and/or distribute this software for any
 * purpose with or without fee is hereby granted, provided that the above
 * copyright notice and this permission notice appear in all copies.
 *
 * THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
 * WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
 * MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR
 * ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
 * WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
 * ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
 * OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.
 *
 * ---
 *
 * The following Lucide icons are derived from the Feather project:
 *
 * airplay, alert-circle, alert-octagon, alert-triangle, aperture, arrow-down-circle, arrow-down-left, arrow-down-right, arrow-down, arrow-left-circle, arrow-left, arrow-right-circle, arrow-right, arrow-up-circle, arrow-up-left, arrow-up-right, arrow-up, at-sign, calendar, cast, check, chevron-down, chevron-left, chevron-right, chevron-up, chevrons-down, chevrons-left, chevrons-right, chevrons-up, circle, clipboard, clock, code, columns, command, compass, corner-down-left, corner-down-right, corner-left-down, corner-left-up, corner-right-down, corner-right-up, corner-up-left, corner-up-right, crosshair, database, divide-circle, divide-square, dollar-sign, download, external-link, feather, frown, hash, headphones, help-circle, info, italic, key, layout, life-buoy, link-2, link, loader, lock, log-in, log-out, maximize, meh, minimize, minimize-2, minus-circle, minus-square, minus, monitor, moon, more-horizontal, more-vertical, move, music, navigation-2, navigation, octagon, pause-circle, percent, plus-circle, plus-square, plus, power, radio, rss, search, server, share, shopping-bag, sidebar, smartphone, smile, square, table-2, tablet, target, terminal, trash-2, trash, triangle, tv, type, upload, x-circle, x-octagon, x-square, x, zoom-in, zoom-out
 *
 * The MIT License (MIT) (for the icons listed above)
 *
 * Copyright (c) 2013-present Cole Bemis
 *
 * Permission is hereby granted, free of charge, to any person obtaining a copy
 * of this software and associated documentation files (the "Software"), to deal
 * in the Software without restriction, including without limitation the rights
 * to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
 * copies of the Software, and to permit persons to whom the Software is
 * furnished to do so, subject to the following conditions:
 *
 * The above copyright notice and this permission notice shall be included in all
 * copies or substantial portions of the Software.
 *
 * THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
 * IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
 * FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
 * AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
 * LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
 * OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
 * SOFTWARE.
 *
 */const r=[["path",{d:"M6 22a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h8a2.4 2.4 0 0 1 1.704.706l3.588 3.588A2.4 2.4 0 0 1 20 8v12a2 2 0 0 1-2 2z"}],["path",{d:"M14 2v5a1 1 0 0 0 1 1h5"}]];re(e,ne({name:"file"},()=>n,{get iconNode(){return r},children:(i,s)=>{var f=K(),o=W(f);X(o,t,"default",{}),z(i,f)},$$slots:{default:!0}}))}function Is(e,t){const n=H(t,["children","$$slots","$$events","$$legacy"]);/**
 * @license lucide-svelte v1.0.1 - ISC
 *
 * ISC License
 *
 * Copyright (c) 2026 Lucide Icons and Contributors
 *
 * Permission to use, copy, modify, and/or distribute this software for any
 * purpose with or without fee is hereby granted, provided that the above
 * copyright notice and this permission notice appear in all copies.
 *
 * THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
 * WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
 * MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR
 * ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
 * WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
 * ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
 * OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.
 *
 * ---
 *
 * The following Lucide icons are derived from the Feather project:
 *
 * airplay, alert-circle, alert-octagon, alert-triangle, aperture, arrow-down-circle, arrow-down-left, arrow-down-right, arrow-down, arrow-left-circle, arrow-left, arrow-right-circle, arrow-right, arrow-up-circle, arrow-up-left, arrow-up-right, arrow-up, at-sign, calendar, cast, check, chevron-down, chevron-left, chevron-right, chevron-up, chevrons-down, chevrons-left, chevrons-right, chevrons-up, circle, clipboard, clock, code, columns, command, compass, corner-down-left, corner-down-right, corner-left-down, corner-left-up, corner-right-down, corner-right-up, corner-up-left, corner-up-right, crosshair, database, divide-circle, divide-square, dollar-sign, download, external-link, feather, frown, hash, headphones, help-circle, info, italic, key, layout, life-buoy, link-2, link, loader, lock, log-in, log-out, maximize, meh, minimize, minimize-2, minus-circle, minus-square, minus, monitor, moon, more-horizontal, more-vertical, move, music, navigation-2, navigation, octagon, pause-circle, percent, plus-circle, plus-square, plus, power, radio, rss, search, server, share, shopping-bag, sidebar, smartphone, smile, square, table-2, tablet, target, terminal, trash-2, trash, triangle, tv, type, upload, x-circle, x-octagon, x-square, x, zoom-in, zoom-out
 *
 * The MIT License (MIT) (for the icons listed above)
 *
 * Copyright (c) 2013-present Cole Bemis
 *
 * Permission is hereby granted, free of charge, to any person obtaining a copy
 * of this software and associated documentation files (the "Software"), to deal
 * in the Software without restriction, including without limitation the rights
 * to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
 * copies of the Software, and to permit persons to whom the Software is
 * furnished to do so, subject to the following conditions:
 *
 * The above copyright notice and this permission notice shall be included in all
 * copies or substantial portions of the Software.
 *
 * THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
 * IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
 * FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
 * AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
 * LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
 * OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
 * SOFTWARE.
 *
 */const r=[["path",{d:"M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.69-.9L9.6 3.9A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z"}]];re(e,ne({name:"folder"},()=>n,{get iconNode(){return r},children:(i,s)=>{var f=K(),o=W(f);X(o,t,"default",{}),z(i,f)},$$slots:{default:!0}}))}function Ls(e,t){const n=H(t,["children","$$slots","$$events","$$legacy"]);/**
 * @license lucide-svelte v1.0.1 - ISC
 *
 * ISC License
 *
 * Copyright (c) 2026 Lucide Icons and Contributors
 *
 * Permission to use, copy, modify, and/or distribute this software for any
 * purpose with or without fee is hereby granted, provided that the above
 * copyright notice and this permission notice appear in all copies.
 *
 * THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
 * WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
 * MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR
 * ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
 * WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
 * ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
 * OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.
 *
 * ---
 *
 * The following Lucide icons are derived from the Feather project:
 *
 * airplay, alert-circle, alert-octagon, alert-triangle, aperture, arrow-down-circle, arrow-down-left, arrow-down-right, arrow-down, arrow-left-circle, arrow-left, arrow-right-circle, arrow-right, arrow-up-circle, arrow-up-left, arrow-up-right, arrow-up, at-sign, calendar, cast, check, chevron-down, chevron-left, chevron-right, chevron-up, chevrons-down, chevrons-left, chevrons-right, chevrons-up, circle, clipboard, clock, code, columns, command, compass, corner-down-left, corner-down-right, corner-left-down, corner-left-up, corner-right-down, corner-right-up, corner-up-left, corner-up-right, crosshair, database, divide-circle, divide-square, dollar-sign, download, external-link, feather, frown, hash, headphones, help-circle, info, italic, key, layout, life-buoy, link-2, link, loader, lock, log-in, log-out, maximize, meh, minimize, minimize-2, minus-circle, minus-square, minus, monitor, moon, more-horizontal, more-vertical, move, music, navigation-2, navigation, octagon, pause-circle, percent, plus-circle, plus-square, plus, power, radio, rss, search, server, share, shopping-bag, sidebar, smartphone, smile, square, table-2, tablet, target, terminal, trash-2, trash, triangle, tv, type, upload, x-circle, x-octagon, x-square, x, zoom-in, zoom-out
 *
 * The MIT License (MIT) (for the icons listed above)
 *
 * Copyright (c) 2013-present Cole Bemis
 *
 * Permission is hereby granted, free of charge, to any person obtaining a copy
 * of this software and associated documentation files (the "Software"), to deal
 * in the Software without restriction, including without limitation the rights
 * to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
 * copies of the Software, and to permit persons to whom the Software is
 * furnished to do so, subject to the following conditions:
 *
 * The above copyright notice and this permission notice shall be included in all
 * copies or substantial portions of the Software.
 *
 * THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
 * IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
 * FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
 * AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
 * LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
 * OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
 * SOFTWARE.
 *
 */const r=[["path",{d:"M15 6a9 9 0 0 0-9 9V3"}],["circle",{cx:"18",cy:"6",r:"3"}],["circle",{cx:"6",cy:"18",r:"3"}]];re(e,ne({name:"git-branch"},()=>n,{get iconNode(){return r},children:(i,s)=>{var f=K(),o=W(f);X(o,t,"default",{}),z(i,f)},$$slots:{default:!0}}))}function Ds(e,t){const n=H(t,["children","$$slots","$$events","$$legacy"]);/**
 * @license lucide-svelte v1.0.1 - ISC
 *
 * ISC License
 *
 * Copyright (c) 2026 Lucide Icons and Contributors
 *
 * Permission to use, copy, modify, and/or distribute this software for any
 * purpose with or without fee is hereby granted, provided that the above
 * copyright notice and this permission notice appear in all copies.
 *
 * THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
 * WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
 * MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR
 * ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
 * WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
 * ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
 * OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.
 *
 * ---
 *
 * The following Lucide icons are derived from the Feather project:
 *
 * airplay, alert-circle, alert-octagon, alert-triangle, aperture, arrow-down-circle, arrow-down-left, arrow-down-right, arrow-down, arrow-left-circle, arrow-left, arrow-right-circle, arrow-right, arrow-up-circle, arrow-up-left, arrow-up-right, arrow-up, at-sign, calendar, cast, check, chevron-down, chevron-left, chevron-right, chevron-up, chevrons-down, chevrons-left, chevrons-right, chevrons-up, circle, clipboard, clock, code, columns, command, compass, corner-down-left, corner-down-right, corner-left-down, corner-left-up, corner-right-down, corner-right-up, corner-up-left, corner-up-right, crosshair, database, divide-circle, divide-square, dollar-sign, download, external-link, feather, frown, hash, headphones, help-circle, info, italic, key, layout, life-buoy, link-2, link, loader, lock, log-in, log-out, maximize, meh, minimize, minimize-2, minus-circle, minus-square, minus, monitor, moon, more-horizontal, more-vertical, move, music, navigation-2, navigation, octagon, pause-circle, percent, plus-circle, plus-square, plus, power, radio, rss, search, server, share, shopping-bag, sidebar, smartphone, smile, square, table-2, tablet, target, terminal, trash-2, trash, triangle, tv, type, upload, x-circle, x-octagon, x-square, x, zoom-in, zoom-out
 *
 * The MIT License (MIT) (for the icons listed above)
 *
 * Copyright (c) 2013-present Cole Bemis
 *
 * Permission is hereby granted, free of charge, to any person obtaining a copy
 * of this software and associated documentation files (the "Software"), to deal
 * in the Software without restriction, including without limitation the rights
 * to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
 * copies of the Software, and to permit persons to whom the Software is
 * furnished to do so, subject to the following conditions:
 *
 * The above copyright notice and this permission notice shall be included in all
 * copies or substantial portions of the Software.
 *
 * THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
 * IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
 * FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
 * AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
 * LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
 * OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
 * SOFTWARE.
 *
 */const r=[["path",{d:"M15 21v-8a1 1 0 0 0-1-1h-4a1 1 0 0 0-1 1v8"}],["path",{d:"M3 10a2 2 0 0 1 .709-1.528l7-6a2 2 0 0 1 2.582 0l7 6A2 2 0 0 1 21 10v9a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"}]];re(e,ne({name:"house"},()=>n,{get iconNode(){return r},children:(i,s)=>{var f=K(),o=W(f);X(o,t,"default",{}),z(i,f)},$$slots:{default:!0}}))}function Fs(e,t){const n=H(t,["children","$$slots","$$events","$$legacy"]);/**
 * @license lucide-svelte v1.0.1 - ISC
 *
 * ISC License
 *
 * Copyright (c) 2026 Lucide Icons and Contributors
 *
 * Permission to use, copy, modify, and/or distribute this software for any
 * purpose with or without fee is hereby granted, provided that the above
 * copyright notice and this permission notice appear in all copies.
 *
 * THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
 * WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
 * MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR
 * ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
 * WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
 * ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
 * OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.
 *
 * ---
 *
 * The following Lucide icons are derived from the Feather project:
 *
 * airplay, alert-circle, alert-octagon, alert-triangle, aperture, arrow-down-circle, arrow-down-left, arrow-down-right, arrow-down, arrow-left-circle, arrow-left, arrow-right-circle, arrow-right, arrow-up-circle, arrow-up-left, arrow-up-right, arrow-up, at-sign, calendar, cast, check, chevron-down, chevron-left, chevron-right, chevron-up, chevrons-down, chevrons-left, chevrons-right, chevrons-up, circle, clipboard, clock, code, columns, command, compass, corner-down-left, corner-down-right, corner-left-down, corner-left-up, corner-right-down, corner-right-up, corner-up-left, corner-up-right, crosshair, database, divide-circle, divide-square, dollar-sign, download, external-link, feather, frown, hash, headphones, help-circle, info, italic, key, layout, life-buoy, link-2, link, loader, lock, log-in, log-out, maximize, meh, minimize, minimize-2, minus-circle, minus-square, minus, monitor, moon, more-horizontal, more-vertical, move, music, navigation-2, navigation, octagon, pause-circle, percent, plus-circle, plus-square, plus, power, radio, rss, search, server, share, shopping-bag, sidebar, smartphone, smile, square, table-2, tablet, target, terminal, trash-2, trash, triangle, tv, type, upload, x-circle, x-octagon, x-square, x, zoom-in, zoom-out
 *
 * The MIT License (MIT) (for the icons listed above)
 *
 * Copyright (c) 2013-present Cole Bemis
 *
 * Permission is hereby granted, free of charge, to any person obtaining a copy
 * of this software and associated documentation files (the "Software"), to deal
 * in the Software without restriction, including without limitation the rights
 * to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
 * copies of the Software, and to permit persons to whom the Software is
 * furnished to do so, subject to the following conditions:
 *
 * The above copyright notice and this permission notice shall be included in all
 * copies or substantial portions of the Software.
 *
 * THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
 * IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
 * FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
 * AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
 * LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
 * OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
 * SOFTWARE.
 *
 */const r=[["path",{d:"M5 12h14"}],["path",{d:"M12 5v14"}]];re(e,ne({name:"plus"},()=>n,{get iconNode(){return r},children:(i,s)=>{var f=K(),o=W(f);X(o,t,"default",{}),z(i,f)},$$slots:{default:!0}}))}function js(e,t){const n=H(t,["children","$$slots","$$events","$$legacy"]);/**
 * @license lucide-svelte v1.0.1 - ISC
 *
 * ISC License
 *
 * Copyright (c) 2026 Lucide Icons and Contributors
 *
 * Permission to use, copy, modify, and/or distribute this software for any
 * purpose with or without fee is hereby granted, provided that the above
 * copyright notice and this permission notice appear in all copies.
 *
 * THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
 * WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
 * MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR
 * ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
 * WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
 * ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
 * OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.
 *
 * ---
 *
 * The following Lucide icons are derived from the Feather project:
 *
 * airplay, alert-circle, alert-octagon, alert-triangle, aperture, arrow-down-circle, arrow-down-left, arrow-down-right, arrow-down, arrow-left-circle, arrow-left, arrow-right-circle, arrow-right, arrow-up-circle, arrow-up-left, arrow-up-right, arrow-up, at-sign, calendar, cast, check, chevron-down, chevron-left, chevron-right, chevron-up, chevrons-down, chevrons-left, chevrons-right, chevrons-up, circle, clipboard, clock, code, columns, command, compass, corner-down-left, corner-down-right, corner-left-down, corner-left-up, corner-right-down, corner-right-up, corner-up-left, corner-up-right, crosshair, database, divide-circle, divide-square, dollar-sign, download, external-link, feather, frown, hash, headphones, help-circle, info, italic, key, layout, life-buoy, link-2, link, loader, lock, log-in, log-out, maximize, meh, minimize, minimize-2, minus-circle, minus-square, minus, monitor, moon, more-horizontal, more-vertical, move, music, navigation-2, navigation, octagon, pause-circle, percent, plus-circle, plus-square, plus, power, radio, rss, search, server, share, shopping-bag, sidebar, smartphone, smile, square, table-2, tablet, target, terminal, trash-2, trash, triangle, tv, type, upload, x-circle, x-octagon, x-square, x, zoom-in, zoom-out
 *
 * The MIT License (MIT) (for the icons listed above)
 *
 * Copyright (c) 2013-present Cole Bemis
 *
 * Permission is hereby granted, free of charge, to any person obtaining a copy
 * of this software and associated documentation files (the "Software"), to deal
 * in the Software without restriction, including without limitation the rights
 * to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
 * copies of the Software, and to permit persons to whom the Software is
 * furnished to do so, subject to the following conditions:
 *
 * The above copyright notice and this permission notice shall be included in all
 * copies or substantial portions of the Software.
 *
 * THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
 * IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
 * FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
 * AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
 * LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
 * OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
 * SOFTWARE.
 *
 */const r=[["path",{d:"M3 12a9 9 0 0 1 9-9 9.75 9.75 0 0 1 6.74 2.74L21 8"}],["path",{d:"M21 3v5h-5"}],["path",{d:"M21 12a9 9 0 0 1-9 9 9.75 9.75 0 0 1-6.74-2.74L3 16"}],["path",{d:"M8 16H3v5"}]];re(e,ne({name:"refresh-cw"},()=>n,{get iconNode(){return r},children:(i,s)=>{var f=K(),o=W(f);X(o,t,"default",{}),z(i,f)},$$slots:{default:!0}}))}function zs(e,t){const n=H(t,["children","$$slots","$$events","$$legacy"]);/**
 * @license lucide-svelte v1.0.1 - ISC
 *
 * ISC License
 *
 * Copyright (c) 2026 Lucide Icons and Contributors
 *
 * Permission to use, copy, modify, and/or distribute this software for any
 * purpose with or without fee is hereby granted, provided that the above
 * copyright notice and this permission notice appear in all copies.
 *
 * THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
 * WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
 * MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR
 * ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
 * WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
 * ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
 * OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.
 *
 * ---
 *
 * The following Lucide icons are derived from the Feather project:
 *
 * airplay, alert-circle, alert-octagon, alert-triangle, aperture, arrow-down-circle, arrow-down-left, arrow-down-right, arrow-down, arrow-left-circle, arrow-left, arrow-right-circle, arrow-right, arrow-up-circle, arrow-up-left, arrow-up-right, arrow-up, at-sign, calendar, cast, check, chevron-down, chevron-left, chevron-right, chevron-up, chevrons-down, chevrons-left, chevrons-right, chevrons-up, circle, clipboard, clock, code, columns, command, compass, corner-down-left, corner-down-right, corner-left-down, corner-left-up, corner-right-down, corner-right-up, corner-up-left, corner-up-right, crosshair, database, divide-circle, divide-square, dollar-sign, download, external-link, feather, frown, hash, headphones, help-circle, info, italic, key, layout, life-buoy, link-2, link, loader, lock, log-in, log-out, maximize, meh, minimize, minimize-2, minus-circle, minus-square, minus, monitor, moon, more-horizontal, more-vertical, move, music, navigation-2, navigation, octagon, pause-circle, percent, plus-circle, plus-square, plus, power, radio, rss, search, server, share, shopping-bag, sidebar, smartphone, smile, square, table-2, tablet, target, terminal, trash-2, trash, triangle, tv, type, upload, x-circle, x-octagon, x-square, x, zoom-in, zoom-out
 *
 * The MIT License (MIT) (for the icons listed above)
 *
 * Copyright (c) 2013-present Cole Bemis
 *
 * Permission is hereby granted, free of charge, to any person obtaining a copy
 * of this software and associated documentation files (the "Software"), to deal
 * in the Software without restriction, including without limitation the rights
 * to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
 * copies of the Software, and to permit persons to whom the Software is
 * furnished to do so, subject to the following conditions:
 *
 * The above copyright notice and this permission notice shall be included in all
 * copies or substantial portions of the Software.
 *
 * THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
 * IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
 * FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
 * AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
 * LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
 * OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
 * SOFTWARE.
 *
 */const r=[["path",{d:"m21 21-4.34-4.34"}],["circle",{cx:"11",cy:"11",r:"8"}]];re(e,ne({name:"search"},()=>n,{get iconNode(){return r},children:(i,s)=>{var f=K(),o=W(f);X(o,t,"default",{}),z(i,f)},$$slots:{default:!0}}))}function Hs(e,t){const n=H(t,["children","$$slots","$$events","$$legacy"]);/**
 * @license lucide-svelte v1.0.1 - ISC
 *
 * ISC License
 *
 * Copyright (c) 2026 Lucide Icons and Contributors
 *
 * Permission to use, copy, modify, and/or distribute this software for any
 * purpose with or without fee is hereby granted, provided that the above
 * copyright notice and this permission notice appear in all copies.
 *
 * THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
 * WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
 * MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR
 * ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
 * WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
 * ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
 * OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.
 *
 * ---
 *
 * The following Lucide icons are derived from the Feather project:
 *
 * airplay, alert-circle, alert-octagon, alert-triangle, aperture, arrow-down-circle, arrow-down-left, arrow-down-right, arrow-down, arrow-left-circle, arrow-left, arrow-right-circle, arrow-right, arrow-up-circle, arrow-up-left, arrow-up-right, arrow-up, at-sign, calendar, cast, check, chevron-down, chevron-left, chevron-right, chevron-up, chevrons-down, chevrons-left, chevrons-right, chevrons-up, circle, clipboard, clock, code, columns, command, compass, corner-down-left, corner-down-right, corner-left-down, corner-left-up, corner-right-down, corner-right-up, corner-up-left, corner-up-right, crosshair, database, divide-circle, divide-square, dollar-sign, download, external-link, feather, frown, hash, headphones, help-circle, info, italic, key, layout, life-buoy, link-2, link, loader, lock, log-in, log-out, maximize, meh, minimize, minimize-2, minus-circle, minus-square, minus, monitor, moon, more-horizontal, more-vertical, move, music, navigation-2, navigation, octagon, pause-circle, percent, plus-circle, plus-square, plus, power, radio, rss, search, server, share, shopping-bag, sidebar, smartphone, smile, square, table-2, tablet, target, terminal, trash-2, trash, triangle, tv, type, upload, x-circle, x-octagon, x-square, x, zoom-in, zoom-out
 *
 * The MIT License (MIT) (for the icons listed above)
 *
 * Copyright (c) 2013-present Cole Bemis
 *
 * Permission is hereby granted, free of charge, to any person obtaining a copy
 * of this software and associated documentation files (the "Software"), to deal
 * in the Software without restriction, including without limitation the rights
 * to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
 * copies of the Software, and to permit persons to whom the Software is
 * furnished to do so, subject to the following conditions:
 *
 * The above copyright notice and this permission notice shall be included in all
 * copies or substantial portions of the Software.
 *
 * THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
 * IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
 * FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
 * AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
 * LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
 * OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
 * SOFTWARE.
 *
 */const r=[["path",{d:"M18 6 6 18"}],["path",{d:"m6 6 12 12"}]];re(e,ne({name:"x"},()=>n,{get iconNode(){return r},children:(i,s)=>{var f=K(),o=W(f);X(o,t,"default",{}),z(i,f)},$$slots:{default:!0}}))}export{sn as $,qi as A,Ms as B,Rs as C,hi as D,Ns as E,Is as F,Ls as G,Ds as H,Os as I,xs as J,Ts as K,Fs as P,js as R,zs as S,Hs as X,z as a,Cn as b,K as c,zi as d,hn as e,W as f,N as g,Ps as h,ks as i,ji as j,Ss as k,mi as l,yi as m,ue as n,ps as o,Pn as p,As as q,is as r,ve as s,Es as t,Le as u,Mt as v,Se as w,Cs as x,$s as y,Ki as z};
