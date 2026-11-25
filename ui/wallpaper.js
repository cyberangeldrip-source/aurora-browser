'use strict';

(() => {
  const canvas = document.getElementById('live-wallpaper');
  if (!canvas) return;

  const reduced = matchMedia('(prefers-reduced-motion: reduce)');
  const light = document.documentElement.dataset.theme === 'light';
  const image = new Image();
  const imageUrl = new URL('aurora-glass.jpg', document.currentScript.src).href;
  const enabled = () => canvas.dataset.enabled === 'true' && !reduced.matches;
  const vertex = `
    attribute vec2 corner;
    varying vec2 screen;
    void main() {
      screen = vec2(corner.x + 1.0, 1.0 - corner.y) * .5;
      gl_Position = vec4(corner, 0.0, 1.0);
    }
  `;
  const fragment = `
    precision highp float;
    varying vec2 screen;
    uniform sampler2D photo;
    uniform vec4 crop;
    uniform float phase;

    void main() {
      vec2 uv = crop.xy + screen * crop.zw;
      vec2 edge = smoothstep(vec2(.0), vec2(.036), min(uv, 1.0 - uv));
      float wave = uv.x * 6.4 - uv.y * 3.1;
      vec2 drift = vec2(sin(wave + phase) * .0032,
                        sin(wave * 1.18 - phase) * .0068);
      vec2 point = uv + drift * edge;
      vec3 color = texture2D(photo, point).rgb;

      float rim = smoothstep(.15, .62, dot(color, vec3(.21, .52, .27)));
      float light = pow(.5 + .5 * sin(wave * 1.61 - phase * 2.0), 9.0);
      color += rim * light * mix(vec3(.035,.099,.135), vec3(.121,.037,.102),
                                .5 + .5 * sin(phase)) * .73;
      gl_FragColor = vec4(color, 1.0);
    }
  `;

  let renderer;
  let raf = 0;
  let timer = 0;
  let previous = 0;
  let elapsed = 0;
  let width, height, left, top, drawnWidth, drawnHeight;
  let canRender = false;

  function makeRenderer() {
    const gl = canvas.getContext('webgl', { alpha: false, antialias: false, depth: false, powerPreference: 'low-power' });
    if (!gl) return null;
    const program = gl.createProgram();
    for (const [kind, source] of [[gl.VERTEX_SHADER, vertex], [gl.FRAGMENT_SHADER, fragment]]) {
      const shader = gl.createShader(kind);
      gl.shaderSource(shader, source);
      gl.compileShader(shader);
      const compiled = gl.getShaderParameter(shader, gl.COMPILE_STATUS);
      if (compiled) gl.attachShader(program, shader);
      gl.deleteShader(shader);
      if (!compiled) return null;
    }
    gl.linkProgram(program);
    if (!gl.getProgramParameter(program, gl.LINK_STATUS)) return null;
    gl.useProgram(program);
    const buffer = gl.createBuffer();
    gl.bindBuffer(gl.ARRAY_BUFFER, buffer);
    gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1, -1, 3, -1, -1, 3]), gl.STATIC_DRAW);
    const corner = gl.getAttribLocation(program, 'corner');
    gl.enableVertexAttribArray(corner);
    gl.vertexAttribPointer(corner, 2, gl.FLOAT, false, 0, 0);
    const texture = gl.createTexture();
    gl.bindTexture(gl.TEXTURE_2D, texture);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
    gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, gl.RGBA, gl.UNSIGNED_BYTE, image);
    const crop = gl.getUniformLocation(program, 'crop');
    const phase = gl.getUniformLocation(program, 'phase');
    return {
      resize() {
        gl.viewport(0, 0, canvas.width, canvas.height);
        gl.uniform4f(crop, -left / drawnWidth, -top / drawnHeight, width / drawnWidth, height / drawnHeight);
      },
      draw() {
        gl.uniform1f(phase, elapsed * Math.PI * 2 / 16000);
        gl.drawArrays(gl.TRIANGLES, 0, 3);
      },
    };
  }

  function resize() {
    width = document.documentElement.clientWidth;
    height = window.innerHeight;
    if (!width || !height || !renderer) return;
    const cover = Math.max(width / image.naturalWidth, height / image.naturalHeight);
    drawnWidth = image.naturalWidth * cover;
    drawnHeight = image.naturalHeight * cover;
    left = (width - drawnWidth) * .5;
    top = (height - drawnHeight) * (light ? .49 : .55);
    const density = Math.min(window.devicePixelRatio || 1, 1.3, Math.sqrt(1400000 / (width * height)));
    canvas.width = Math.max(1, Math.round(width * density));
    canvas.height = Math.max(1, Math.round(height * density));
    renderer.resize();
  }

  function schedule() {
    if (!canRender || raf || timer) return;
    timer = setTimeout(() => {
      timer = 0;
      if (canRender) raf = requestAnimationFrame(draw);
    }, 25);
  }

  function draw(now) {
    raf = 0;
    if (!canRender || !renderer) return;
    const dt = previous ? Math.min(now - previous, 60) : 16;
    previous = now;
    elapsed = (elapsed + dt) % 16000;
    renderer.draw();
    canvas.hidden = false;
    schedule();
  }

  function visibility() {
    canRender = !document.hidden && enabled() && Boolean(renderer);
    clearTimeout(timer);
    cancelAnimationFrame(raf);
    timer = raf = 0;
    previous = 0;
    if (!enabled() || !renderer) canvas.hidden = true;
    if (canRender) raf = requestAnimationFrame(draw);
  }

  function start() {
    if (!enabled()) return visibility();
    if (!image.getAttribute('src')) image.src = imageUrl;
    if (!image.complete || !image.naturalWidth) return;
    try { renderer ||= makeRenderer(); } catch { renderer = null; }
    resize();
    visibility();
  }

  canvas.addEventListener('webglcontextlost', (e) => {
    e.preventDefault();
    renderer = null;
    visibility();
  });
  canvas.addEventListener('webglcontextrestored', start);
  window.addEventListener('resize', () => { resize(); schedule(); });
  document.addEventListener('visibilitychange', visibility);
  window.addEventListener('pagehide', () => {
    canRender = false;
    clearTimeout(timer);
    cancelAnimationFrame(raf);
    timer = raf = 0;
  });
  window.addEventListener('pageshow', start);
  reduced.addEventListener('change', start);
  new MutationObserver(start).observe(canvas, { attributeFilter: ['data-enabled'] });
  image.addEventListener('load', start, { once: true });
  start();
})();
