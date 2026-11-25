'use strict';

const liquidGlass = (() => {
  const vertex = `
    attribute vec2 position;
    varying vec2 uv;
    void main() {
      uv = position * 0.5 + 0.5;
      gl_Position = vec4(position, 0.0, 1.0);
    }
  `;
  // Sample a separable Gaussian kernel at reduced resolution.
  const gaussian = `
    precision highp float;
    varying vec2 uv;
    uniform sampler2D source;
    uniform vec2 stepSize;
    void main() {
      vec3 sum = texture2D(source, uv).rgb;
      float total = 1.0;
      for (int i = 1; i <= 8; i++) {
        float tap = float(i);
        float weight = exp(-tap * tap / 17.4);
        vec2 offset = stepSize * tap;
        sum += (texture2D(source, uv + offset).rgb + texture2D(source, uv - offset).rgb) * weight;
        total += 2.0 * weight;
      }
      gl_FragColor = vec4(sum / total, 1.0);
    }
  `;
  const glass = `
    precision highp float;
    varying vec2 uv;
    uniform sampler2D scene;
    uniform sampler2D softScene;
    uniform vec2 viewport;
    uniform vec4 bounds;
    uniform float radius;

    vec2 sceneUV(vec2 point) { return vec2(point.x / viewport.x, 1.0 - point.y / viewport.y); }

    vec3 radialSample(vec2 center, vec2 normal, float distance) {
      vec2 point = center + normal * distance;
      return texture2D(scene, sceneUV(point)).rgb * 0.5
           + texture2D(scene, sceneUV(point + normal * 3.5)).rgb * 0.25
           + texture2D(scene, sceneUV(point - normal * 3.5)).rgb * 0.25;
    }

    void main() {
      vec2 size = bounds.zw;
      vec2 point = vec2(uv.x, 1.0 - uv.y) * size;
      vec2 edge = point - clamp(point, vec2(radius), size - vec2(radius));
      float lengthToCenter = length(edge);
      vec2 normal = edge / max(lengthToCenter, 0.01);
      float inside = radius - lengthToCenter;
      float rim = 1.0 - smoothstep(0.0, min(21.0, radius), inside);
      float lower = smoothstep(0.24, 0.89, normal.y)
                  * smoothstep(size.y * 0.47, size.y * 0.85, point.y);
      float lens = rim * lower;
      vec2 center = bounds.xy + point;
      vec3 color = texture2D(softScene, sceneUV(center)).rgb;
      if (lens > 0.01) {
        float warp = 4.0 + lens * lens * 16.0;
        vec3 channels = vec3(
          radialSample(center, normal, warp + lens * 8.0).r,
          radialSample(center, normal, warp).g,
          radialSample(center, normal, warp - lens * 7.0).b
        );
        color = mix(color, channels, lens * 0.94);
      }
      gl_FragColor = vec4(color, 1.0);
    }
  `;

  let renderer;
  let owner;
  let key;
  let frame;
  let cache;
  let request;
  let motion;
  let closing;
  let acceptUntil = 0;
  let idleTimer;
  let pendingDraw;
  const sampleAge = 3500;
  const resize = new ResizeObserver(() => queueDraw());
  const reducedMotion = matchMedia('(prefers-reduced-motion: reduce)');
  const animated = () => !document.documentElement.classList.contains('no-motion') && !reducedMotion.matches;
  const closed = { transform: 'translateY(-6px) scale(.962, .94)', opacity: 0 };
  const opened = { transform: 'none', opacity: 1 };
  const ease = 'cubic-bezier(.18,.82,.24,1)';

  function makeRenderer() {
    const canvas = document.createElement('canvas');
    canvas.className = 'glass-optics';
    canvas.setAttribute('aria-hidden', 'true');
    const gl = canvas.getContext('webgl', { alpha: false, antialias: false, depth: false, powerPreference: 'low-power' });
    if (!gl) return null;

    function program(source, uniforms) {
      const result = gl.createProgram();
      const shaders = [[gl.VERTEX_SHADER, vertex], [gl.FRAGMENT_SHADER, source]].map(([type, code]) => {
        const shader = gl.createShader(type);
        gl.shaderSource(shader, code);
        gl.compileShader(shader);
        if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) throw new Error(gl.getShaderInfoLog(shader));
        gl.attachShader(result, shader);
        return shader;
      });
      gl.linkProgram(result);
      for (const shader of shaders) gl.deleteShader(shader);
      if (!gl.getProgramParameter(result, gl.LINK_STATUS)) throw new Error(gl.getProgramInfoLog(result));
      return {
        program: result,
        position: gl.getAttribLocation(result, 'position'),
        uniforms: Object.fromEntries(uniforms.map(name => [name, gl.getUniformLocation(result, name)])),
      };
    }

    function texture() {
      const target = gl.createTexture();
      gl.bindTexture(gl.TEXTURE_2D, target);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
      return target;
    }

    function target() {
      const textureId = texture();
      const fbo = gl.createFramebuffer();
      gl.bindFramebuffer(gl.FRAMEBUFFER, fbo);
      gl.framebufferTexture2D(gl.FRAMEBUFFER, gl.COLOR_ATTACHMENT0, gl.TEXTURE_2D, textureId, 0);
      return { textureId, fbo };
    }

    function drawWith(shader) {
      gl.useProgram(shader.program);
      gl.enableVertexAttribArray(shader.position);
      gl.vertexAttribPointer(shader.position, 2, gl.FLOAT, false, 0, 0);
      return shader.uniforms;
    }

    function useTexture(unit, item) {
      gl.activeTexture(gl.TEXTURE0 + unit);
      gl.bindTexture(gl.TEXTURE_2D, item);
    }

    const blurProgram = program(gaussian, ['source', 'stepSize']);
    const glassProgram = program(glass, ['scene', 'softScene', 'viewport', 'bounds', 'radius']);
    const buffer = gl.createBuffer();
    gl.bindBuffer(gl.ARRAY_BUFFER, buffer);
    gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1, -1, 3, -1, -1, 3]), gl.STATIC_DRAW);
    const original = texture();
    const horizontal = target();
    const vertical = target();
    let uploaded;
    let blurred;
    let previousBlur;
    let width;
    let height;

    function soften(sample, blur) {
      if (uploaded !== sample) {
        useTexture(0, original);
        gl.pixelStorei(gl.UNPACK_FLIP_Y_WEBGL, true);
        gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, gl.RGBA, gl.UNSIGNED_BYTE, sample.image);
        uploaded = sample;
        blurred = null;
        const scale = Math.min(0.53, 550 / Math.max(sample.width, sample.height));
        width = Math.max(1, Math.round(sample.width * scale));
        height = Math.max(1, Math.round(sample.height * scale));
        for (const layer of [horizontal, vertical]) {
          useTexture(0, layer.textureId);
          gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, width, height, 0, gl.RGBA, gl.UNSIGNED_BYTE, null);
        }
      }
      if (blurred === sample && previousBlur === blur) return;
      const uniforms = drawWith(blurProgram);
      gl.uniform1i(uniforms.source, 0);
      gl.viewport(0, 0, width, height);
      const spread = blur * 0.165;
      gl.bindFramebuffer(gl.FRAMEBUFFER, horizontal.fbo);
      useTexture(0, original);
      gl.uniform2f(uniforms.stepSize, spread / sample.width, 0);
      gl.drawArrays(gl.TRIANGLES, 0, 3);
      gl.bindFramebuffer(gl.FRAMEBUFFER, vertical.fbo);
      useTexture(0, horizontal.textureId);
      gl.uniform2f(uniforms.stepSize, 0, spread / sample.height);
      gl.drawArrays(gl.TRIANGLES, 0, 3);
      blurred = sample;
      previousBlur = blur;
    }

    return {
      canvas,
      draw(sample, rect, radius, blur) {
        soften(sample, blur);
        const ratio = Math.min(window.devicePixelRatio || 1, 1.35, 960 / Math.max(rect.width, rect.height));
        const outputWidth = Math.max(1, Math.round(rect.width * ratio));
        const outputHeight = Math.max(1, Math.round(rect.height * ratio));
        if (canvas.width !== outputWidth) canvas.width = outputWidth;
        if (canvas.height !== outputHeight) canvas.height = outputHeight;
        gl.bindFramebuffer(gl.FRAMEBUFFER, null);
        gl.viewport(0, 0, outputWidth, outputHeight);
        const uniforms = drawWith(glassProgram);
        useTexture(0, original);
        useTexture(1, vertical.textureId);
        gl.uniform1i(uniforms.scene, 0);
        gl.uniform1i(uniforms.softScene, 1);
        gl.uniform2f(uniforms.viewport, sample.width, sample.height);
        gl.uniform4f(uniforms.bounds, rect.left, rect.top - sample.top, rect.width, rect.height);
        gl.uniform1f(uniforms.radius, radius);
        gl.drawArrays(gl.TRIANGLES, 0, 3);
      },
      release() {
        if (!uploaded) return;
        for (const item of [original, horizontal.textureId, vertical.textureId]) {
          useTexture(0, item);
          gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, 1, 1, 0, gl.RGBA, gl.UNSIGNED_BYTE, null);
        }
        blurred = uploaded = null;
      },
    };
  }

  function paint() {
    if (!owner || !frame || !renderer || closing) return;
    const rect = { left: 0, top: 0, width: owner.offsetWidth, height: owner.offsetHeight };
    if (rect.width < 4 || rect.height < 4) return;
    for (let node = owner; node; node = node.offsetParent) {
      rect.left += node.offsetLeft + (node.offsetParent?.clientLeft ?? 0);
      rect.top += node.offsetTop + (node.offsetParent?.clientTop ?? 0);
    }
    const style = getComputedStyle(owner);
    const radius = Math.min(parseFloat(style.borderTopLeftRadius), rect.height / 2, rect.width / 2) || 16;
    const blur = parseFloat(style.getPropertyValue('--blur')) || 22;
    renderer.draw(frame, rect, radius, blur);
    owner.classList.add('glass-ready');
  }

  function queueDraw() {
    if (!owner || !frame || pendingDraw || closing) return;
    pendingDraw = requestAnimationFrame(() => {
      pendingDraw = null;
      paint();
    });
  }

  function fresh(pageKey) {
    return cache?.key === pageKey && performance.now() - cache.time < sampleAge ? cache.frame : null;
  }

  function releaseWhenIdle() {
    clearTimeout(idleTimer);
    idleTimer = setTimeout(() => {
      if (owner) return;
      cache = null;
      renderer?.release();
    }, sampleAge);
  }

  function setPage(pageKey) {
    if (cache?.key !== pageKey) {
      cache = null;
      if (!owner) renderer?.release();
    }
    if (request?.key !== pageKey) request = null;
  }

  async function prepare(pageKey, readFrame) {
    if (!pageKey || !readFrame || renderer === null || fresh(pageKey) || request?.key === pageKey) return;
    const job = { key: pageKey };
    request = job;
    try {
      const sample = await readFrame();
      if (request !== job || !sample?.image) return;
      const image = new Image();
      image.src = 'data:image/jpeg;base64,' + sample.image;
      await image.decode();
      if (request !== job) return;
      const decoded = { image, width: sample.width, height: sample.height, top: sample.top };
      cache = { key: pageKey, time: performance.now(), frame: decoded };
      if (owner && key === pageKey && !closing && performance.now() < acceptUntil) {
        frame = decoded;
        queueDraw();
      }
      if (!owner) releaseWhenIdle();
    } catch {
      // The translucent CSS surface still works if WebGL or the page sample is unavailable.
    } finally {
      if (request === job) request = null;
    }
  }

  function clearSurface() {
    motion?.cancel();
    motion = null;
    if (!owner) return;
    resize.unobserve(owner);
    owner.classList.remove('glass-ready');
    const layer = owner.closest('#panel-overlay') ?? owner;
    layer.classList.remove('glass-leaving');
    layer.inert = false;
    renderer?.canvas.remove();
    owner = null;
    closing = false;
    frame = null;
    releaseWhenIdle();
  }

  function hide() {
    if (!owner || closing) return;
    if (!animated()) return clearSurface();
    const target = owner;
    const layer = target.closest('#panel-overlay') ?? target;
    layer.classList.add('glass-leaving');
    layer.inert = true;
    const style = getComputedStyle(target);
    const from = { transform: style.transform, opacity: style.opacity };
    motion?.cancel();
    closing = true;
    const exit = target.animate([from, closed], { duration: 126, easing: ease });
    motion = exit;
    exit.finished.then(() => { if (motion === exit) clearSurface(); }, () => {});
  }

  function show(surface, pageKey, readFrame) {
    if (!surface) return hide();
    if (owner === surface && !closing) {
      if (key !== pageKey) {
        owner.classList.remove('glass-ready');
        frame = null;
        key = pageKey;
        acceptUntil = 0;
        prepare(pageKey, readFrame);
      }
      return;
    }
    let from = closed;
    const reopening = owner === surface && closing && key === pageKey;
    if (reopening) {
      const style = getComputedStyle(surface);
      from = { transform: style.transform, opacity: style.opacity };
      const layer = owner.closest('#panel-overlay') ?? owner;
      layer.classList.remove('glass-leaving');
      layer.inert = false;
      motion?.cancel();
      motion = null;
      closing = false;
    } else {
      clearSurface();
      owner = surface;
      key = pageKey;
    }
    const plays = animated();
    acceptUntil = plays ? performance.now() + 95 : 0;

    if (renderer === undefined) {
      try { renderer = makeRenderer(); } catch { renderer = null; }
    }
    if (renderer) {
      if (!reopening) {
        owner.prepend(renderer.canvas);
        resize.observe(owner);
      }
      frame ??= fresh(pageKey);
      if (!frame) prepare(pageKey, readFrame);
      else if (!owner.classList.contains('glass-ready')) paint();
    }

    if (plays) {
      const animation = owner.animate([from, opened], { duration: 197, easing: ease });
      motion = animation;
      animation.finished.then(() => {
        if (motion === animation) motion = null;
      }, () => {});
    }
  }

  return { show, prepare, setPage, animated, repaint: queueDraw };
})();
