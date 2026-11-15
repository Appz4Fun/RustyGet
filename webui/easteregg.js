/*
 * This file is part of RustyGet.
 *
 * This program is free software; you can redistribute it and/or modify
 * it under the terms of the GNU General Public License as published by
 * the Free Software Foundation; either version 2 of the License, or
 * (at your option) any later version.
 *
 * This program is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 * GNU General Public License for more details.
 *
 * You should have received a copy of the GNU General Public License
 * along with this program.  If not, see <https://www.gnu.org/licenses/>.
 */

/*
 * Click the RustyGet logo: confetti bursts out of it, and for the next 30
 * seconds fireworks bloom wherever the mouse (or a finger) goes. One canvas
 * over the page that ignores the pointer, removed when the show ends.
 */
var EasterEgg = (new function()
{
	'use strict';

	var DURATION_MS = 30000;
	var COLORS = ['#f97316', '#ea580c', '#facc15', '#22d3ee', '#a78bfa', '#4ade80', '#f472b6', '#ffffff'];

	var canvas = null;
	var ctx = null;
	var particles = [];
	var endAt = 0;
	var frame = 0;
	var lastBurst = 0;
	var reduced = false;

	function rand(a, b) { return a + Math.random() * (b - a); }
	function pick(list) { return list[Math.floor(Math.random() * list.length)]; }

	function ensureCanvas()
	{
		if (canvas) return;
		canvas = document.createElement('canvas');
		canvas.id = 'EasterEggCanvas';
		canvas.setAttribute('aria-hidden', 'true');
		canvas.style.cssText = 'position:fixed;inset:0;width:100%;height:100%;pointer-events:none;z-index:100000';
		document.body.appendChild(canvas);
		ctx = canvas.getContext('2d');
		resize();
		window.addEventListener('resize', resize);
		document.addEventListener('mousemove', onMove, { passive: true });
		document.addEventListener('touchmove', onTouch, { passive: true });
		document.addEventListener('touchstart', onTouch, { passive: true });
	}

	function resize()
	{
		if (!canvas) return;
		var dpr = window.devicePixelRatio || 1;
		canvas.width = Math.floor(window.innerWidth * dpr);
		canvas.height = Math.floor(window.innerHeight * dpr);
		ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
	}

	function teardown()
	{
		if (!canvas) return;
		cancelAnimationFrame(frame);
		window.removeEventListener('resize', resize);
		document.removeEventListener('mousemove', onMove);
		document.removeEventListener('touchmove', onTouch);
		document.removeEventListener('touchstart', onTouch);
		canvas.parentNode && canvas.parentNode.removeChild(canvas);
		canvas = null;
		ctx = null;
		particles = [];
	}

	function confetti(x, y, count, down)
	{
		for (var i = 0; i < count; i++)
		{
			var angle = down ? rand(Math.PI * 0.2, Math.PI * 0.8) : rand(-Math.PI * 0.95, -Math.PI * 0.05) + rand(-0.3, 0.3);
			var speed = down ? rand(1, 5) : rand(6, 16);
			particles.push({
				kind: 'confetti', x: x, y: y,
				vx: Math.cos(angle) * speed + (down ? 0 : rand(2, 6)), vy: Math.sin(angle) * speed,
				w: rand(5, 10), h: rand(8, 14), rot: rand(0, Math.PI * 2), vr: rand(-0.3, 0.3),
				color: pick(COLORS), life: rand(180, 300), age: 0
			});
		}
	}

	function firework(x, y)
	{
		var count = reduced ? 18 : 42;
		var color = pick(COLORS);
		var color2 = pick(COLORS);
		for (var i = 0; i < count; i++)
		{
			var angle = (i / count) * Math.PI * 2 + rand(-0.05, 0.05);
			var speed = rand(2.2, 5.2);
			particles.push({
				kind: 'spark', x: x, y: y, px: x, py: y,
				vx: Math.cos(angle) * speed, vy: Math.sin(angle) * speed,
				color: i % 3 ? color : color2, life: rand(45, 75), age: 0, size: rand(1.5, 2.8)
			});
		}
	}

	function onMove(e)
	{
		burstAt(e.clientX, e.clientY);
	}

	function onTouch(e)
	{
		if (e.touches && e.touches[0]) burstAt(e.touches[0].clientX, e.touches[0].clientY);
	}

	function burstAt(x, y)
	{
		var now = Date.now();
		if (now > endAt || now - lastBurst < (reduced ? 400 : 140)) return;
		lastBurst = now;
		firework(x, y);
	}

	function step()
	{
		var w = window.innerWidth, h = window.innerHeight;
		ctx.clearRect(0, 0, w, h);
		var alive = [];
		for (var i = 0; i < particles.length; i++)
		{
			var p = particles[i];
			p.age++;
			if (p.age > p.life) continue;
			var fade = 1 - p.age / p.life;
			if (p.kind === 'confetti')
			{
				p.vx *= 0.985;
				p.vy = p.vy * 0.985 + 0.28;
				p.x += p.vx;
				p.y += p.vy;
				p.rot += p.vr;
				if (p.y > h + 20) continue;
				ctx.save();
				ctx.globalAlpha = Math.min(1, fade * 1.6);
				ctx.translate(p.x, p.y);
				ctx.rotate(p.rot);
				ctx.fillStyle = p.color;
				ctx.fillRect(-p.w / 2, -p.h / 2 * Math.abs(Math.cos(p.rot * 2)), p.w, p.h * Math.abs(Math.cos(p.rot * 2)) + 1);
				ctx.restore();
			}
			else
			{
				p.px = p.x;
				p.py = p.y;
				p.vx *= 0.96;
				p.vy = p.vy * 0.96 + 0.05;
				p.x += p.vx;
				p.y += p.vy;
				ctx.globalAlpha = fade;
				ctx.strokeStyle = p.color;
				ctx.lineWidth = p.size;
				ctx.lineCap = 'round';
				ctx.beginPath();
				ctx.moveTo(p.px, p.py);
				ctx.lineTo(p.x, p.y);
				ctx.stroke();
			}
			alive.push(p);
		}
		ctx.globalAlpha = 1;
		particles = alive;
		if (Date.now() < endAt || particles.length)
		{
			frame = requestAnimationFrame(step);
		}
		else
		{
			teardown();
		}
	}

	// Starts (or extends) the show from the element that was clicked.
	this.fire = function(el)
	{
		reduced = !!(window.matchMedia && window.matchMedia('(prefers-reduced-motion: reduce)').matches);
		var running = !!canvas;
		ensureCanvas();
		var rect = el && el.getBoundingClientRect ? el.getBoundingClientRect() : { left: 20, top: 20, width: 40, height: 40 };
		var x = rect.left + rect.width / 2, y = rect.top + rect.height / 2;
		confetti(x, y, reduced ? 60 : 220);
		// a second wave across the top of the window
		if (!reduced)
		{
			for (var i = 1; i <= 4; i++)
			{
				confetti(window.innerWidth * i / 5, -10, 40, true);
			}
		}
		endAt = Date.now() + DURATION_MS;
		if (!running) frame = requestAnimationFrame(step);
	};

	this.isRunning = function() { return !!canvas; };
	this.particleCount = function() { return particles.length; };
}());
