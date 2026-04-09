<script lang="ts">
	import { browser } from '$app/environment';
	import { invoke } from '@tauri-apps/api/core';
	import { flip } from 'svelte/animate';
	import { cubicOut } from 'svelte/easing';

	type PdfFile = {
		id: string;
		name: string;
		size: number; // en octets
		path: string;
		nbPages: number;
	};

	type Page = {
		pageNumber: number;
		rotation: number;
	};

	let files = $state<PdfFile[]>([
		// {
		// 	id: crypto.randomUUID(),
		// 	name: 'contrat-client.pdf',
		// 	size: 248512,
		// 	path: 'mock://contrat-client.pdf',
		// 	nbPages: 3
		// },
		// {
		// 	id: crypto.randomUUID(),
		// 	name: 'facture-avril.pdf',
		// 	size: 104320,
		// 	path: 'mock://facture-avril.pdf',
		// 	nbPages: 1
		// },
		// {
		// 	id: crypto.randomUUID(),
		// 	name: 'presentation-projet.pdf',
		// 	size: 890112,
		// 	path: 'mock://presentation-projet.pdf',
		// 	nbPages: 8
		// },
		// {
		// 	id: crypto.randomUUID(),
		// 	name: 'rapport-2025.pdf',
		// 	size: 1520430,
		// 	path: 'mock://rapport-2025.pdf',
		// 	nbPages: 12
		// }
	]);
	let operationStatus = $state<string | null>(null);
	let operationError = $state<string | null>(null);
	let selectedPages = $state<Page[]>([]);
	let isMerging = $state(false);
	let isSplitting = $state(false);
	let thumbnails = $state<Record<string, string>>({});
	let isMock = $state(false);

	function thumbnailKey(fileId: string, pageNumber: number) {
		return `${fileId}:${pageNumber}`;
	}

	async function renderPagesForFile(file: PdfFile, pageNumbers: number[]) {
		if (!browser) return;
		if (isMock) {
			const nextThumbnails: Record<string, string> = {};

			for (const pageNumber of pageNumbers) {
				const canvas = document.createElement('canvas');
				canvas.width = 220;
				canvas.height = 300;

				const context = canvas.getContext('2d');
				if (!context) continue;

				context.fillStyle = '#ffffff';
				context.fillRect(0, 0, canvas.width, canvas.height);

				context.fillStyle = '#2563eb';
				context.fillRect(0, 0, canvas.width, 44);

				context.fillStyle = '#ffffff';
				context.font = 'bold 14px sans-serif';
				context.fillText(file.name.slice(0, 20), 10, 28);

				context.fillStyle = '#0f172a';
				context.font = 'bold 28px sans-serif';
				context.fillText(String(pageNumber), 96, 170);

				nextThumbnails[thumbnailKey(file.id, pageNumber)] = canvas.toDataURL('image/png');
			}

			thumbnails = { ...thumbnails, ...nextThumbnails };
		} else {
			try {
				const pdfjs = await import('pdfjs-dist');
				const pdfWorker = await import('pdfjs-dist/build/pdf.worker.min.mjs?url');
				const { readFile } = await import('@tauri-apps/plugin-fs');
				pdfjs.GlobalWorkerOptions.workerSrc = pdfWorker.default;

				const fileData = await readFile(file.path);
				const pdfData = fileData instanceof Uint8Array ? fileData : new Uint8Array(fileData);
				const pdfDoc = await pdfjs.getDocument({ data: pdfData }).promise;
				const nextThumbnails: Record<string, string> = {};

				for (const pageNumber of pageNumbers) {
					try {
						const page = await pdfDoc.getPage(pageNumber);
						const canvas = document.createElement('canvas');
						const context = canvas.getContext('2d');
						if (!context) continue;

						const viewport = page.getViewport({ scale: 0.35 });
						canvas.width = Math.floor(viewport.width);
						canvas.height = Math.floor(viewport.height);

						await page.render({
							canvasContext: context,
							viewport,
							canvas
						}).promise;

						nextThumbnails[thumbnailKey(file.id, pageNumber)] = canvas.toDataURL('image/png');
					} catch (pageError) {
						operationError = pageError instanceof Error ? pageError.message : String(pageError);
					}
				}

				thumbnails = {
					...thumbnails,
					...nextThumbnails
				};
			} catch (error) {
				operationError = error instanceof Error ? error.message : String(error);
			}
		}
	}

	async function refreshThumbnails(currentFiles: PdfFile[]) {
		thumbnails = {};

		if (currentFiles.length === 0) return;

		if (currentFiles.length === 1) {
			const file = currentFiles[0];
			const pages = Array.from({ length: file.nbPages }, (_, i) => i + 1);
			await renderPagesForFile(file, pages);
			return;
		}

		for (const file of currentFiles) {
			await renderPagesForFile(file, [1]);
		}
	}

	async function removeFile(id: string) {
		files = files.filter((file) => file.id !== id);
		selectedPages = [];
		await refreshThumbnails(files);
	}

	async function selectPdfFiles() {
		if (!browser) return;

		const { open } = await import('@tauri-apps/plugin-dialog');
		const { stat } = await import('@tauri-apps/plugin-fs');

		const selected = await open({
			multiple: true,
			filters: [{ name: 'PDF', extensions: ['pdf'] }]
		});

		if (!selected) return;

		const paths = Array.isArray(selected) ? selected : [selected];
		const newFiles: PdfFile[] = await Promise.all(
			paths.map(async (path) => {
				const normalized = path.replaceAll('\\', '/');
				const name = normalized.split('/').pop() ?? 'unknown.pdf';

				const fileInfo = await stat(path);
				const size = typeof fileInfo.size === 'number' ? fileInfo.size : 0;
				const nbPages = await invoke('get_pdf_page_count', { inputPath: path }).then(
					(result) => (result as { page_count?: number }).page_count ?? 0
				);
				return {
					id: crypto.randomUUID(),
					name,
					size,
					path,
					nbPages
				};
			})
		);
		const uniqueNewFiles = newFiles.filter(
			(newFile) => !files.some((existingFile) => existingFile.path === newFile.path)
		);
		files = [...files, ...uniqueNewFiles];
		selectedPages = [];
		await refreshThumbnails(files);
	}

	async function runMergePdfs() {
		if (files.length < 2 || isMerging) return;
		if (!browser) return;

		operationStatus = null;
		operationError = null;
		isMerging = true;

		try {
			const { save } = await import('@tauri-apps/plugin-dialog');
			const outputPath = await save({
				defaultPath: 'merged.pdf',
				filters: [{ name: 'PDF', extensions: ['pdf'] }]
			});

			if (!outputPath) {
				return;
			}

			const inputPaths = files.map((file) => file.path);
			const result = await invoke('merge_pdfs', {
				inputPaths,
				outputPath
			});

			operationStatus = (result as { message?: string }).message ?? 'Fusion terminée.';
			clearOperationMessages();
		} catch (error) {
			operationError = error instanceof Error ? error.message : String(error);
			clearOperationMessages();
		} finally {
			isMerging = false;
			clearOperationMessages();
		}
	}

	async function runSplitPdf() {
		if (files.length !== 1 || isSplitting || selectedPages.length === 0) return;
		if (!browser) return;

		operationStatus = null;
		operationError = null;
		isSplitting = true;

		try {
			const { save } = await import('@tauri-apps/plugin-dialog');
			const outputPath = await save({
				defaultPath: 'splitted.pdf',
				filters: [{ name: 'PDF', extensions: ['pdf'] }]
			});

			if (!outputPath) {
				return;
			}

			const result = await invoke('split_pdf', {
				inputPath: files[0].path,
				selectedPages: selectedPages.map((page) => ({
					pageNumber: page.pageNumber,
					rotation: normalizeRotation(page.rotation)
				})),
				outputPath
			});

			operationStatus = (result as { message?: string }).message ?? 'Split terminé.';
		} catch (error) {
			operationError = error instanceof Error ? error.message : String(error);
		} finally {
			isSplitting = false;
			clearOperationMessages();
		}
	}

	function clearOperationMessages() {
		setTimeout(() => {
			operationStatus = null;
			operationError = null;
		}, 4000);
	}

	function selectPage(pageNumber: number) {
		const alreadySelected = selectedPages.some((p) => p.pageNumber === pageNumber);

		if (alreadySelected) {
			selectedPages = selectedPages.filter((p) => p.pageNumber !== pageNumber);
		} else {
			selectedPages = [...selectedPages, { pageNumber, rotation: 0 }];
		}
	}

	function selectAllPages() {
		if (files.length === 0) return;
		const allPageNumbers = Array.from({ length: files[0].nbPages }, (_, i) => i + 1);
		const newPages = allPageNumbers
			.filter((pageNum) => !selectedPages.some((p) => p.pageNumber === pageNum))
			.map((pageNum) => ({ pageNumber: pageNum, rotation: 0 }));
		selectedPages = [...selectedPages, ...newPages];
	}

	function movePage(pageNumber: number, direction: -1 | 1) {
		const index = selectedPages.findIndex((p) => p.pageNumber === pageNumber);
		const targetIndex = index + direction;

		if (index < 0 || targetIndex < 0 || targetIndex >= selectedPages.length) {
			return;
		}

		const [page] = selectedPages.splice(index, 1);
		selectedPages.splice(targetIndex, 0, page);
		selectedPages = [...selectedPages];
	}

	function rotatePage(pageNumber: number, angle: -90 | 90) {
		const index = selectedPages.findIndex((p) => p.pageNumber === pageNumber);
		if (index < 0) {
			return;
		}

		selectedPages[index].rotation = selectedPages[index].rotation + angle;
		selectedPages = [...selectedPages];
	}

	function moveFile(id: string, direction: -1 | 1) {
		const index = files.findIndex((file) => file.id === id);
		const targetIndex = index + direction;
		if (index < 0 || targetIndex < 0 || targetIndex >= files.length) {
			return;
		}
		const [file] = files.splice(index, 1);
		files.splice(targetIndex, 0, file);
		files = [...files];
	}

	function normalizeRotation(rotation: number) {
		return ((rotation % 360) + 360) % 360;
	}

	function formatBytes(value: number) {
		if (value <= 0) return '0 B';
		if (value < 1024) return `${value} B`;
		if (value < 1024 * 1024) return `${(value / 1024).toFixed(1)} KB`;
		return `${(value / (1024 * 1024)).toFixed(1)} MB`;
	}
</script>

<main class="mx-auto max-w-6xl space-y-6 p-6">
	<header class="rounded-2xl border border-slate-200 bg-white p-4 shadow-sm">
		<div class="flex flex-wrap items-center justify-between gap-3">
			<div>
				<h1 class="text-xl font-semibold text-slate-900">Bienvenu sur NMPDF</h1>
				<p class="text-sm text-slate-600">{files.length} fichier(s) chargé(s)</p>
			</div>
			<div class="flex flex-wrap gap-2">
				<button
					type="button"
					class="rounded-md border border-green-600 bg-green-600 px-3 py-2 text-sm font-medium text-white transition hover:cursor-pointer hover:bg-green-700"
					onclick={selectPdfFiles}
				>
					Ajouter PDF
				</button>
				<button
					type="button"
					disabled={files.length < 2}
					class="rounded-md border border-blue-600 bg-blue-600 px-3 py-2 text-sm font-medium text-white transition hover:cursor-pointer hover:bg-blue-700 disabled:cursor-not-allowed disabled:opacity-50"
					onclick={runMergePdfs}
				>
					{isMerging ? 'Fusion en cours...' : 'Fusionner'}
				</button>
				<button
					type="button"
					disabled={files.length !== 1 || selectedPages.length === 0}
					class="rounded-md border border-blue-600 bg-blue-600 px-3 py-2 text-sm font-medium text-white transition hover:cursor-pointer hover:bg-blue-700 disabled:cursor-not-allowed disabled:opacity-50"
					onclick={runSplitPdf}
				>
					{isSplitting ? 'Split en cours...' : 'Modifier le fichier'}
				</button>
			</div>
		</div>

		{#if operationStatus}
			<p
				class="mt-3 rounded-md border border-emerald-300 bg-emerald-50 px-3 py-2 text-sm text-emerald-700"
			>
				{operationStatus}
			</p>
		{/if}

		{#if operationError}
			<p class="mt-3 rounded-md border border-rose-300 bg-rose-50 px-3 py-2 text-sm text-rose-700">
				{operationError}
			</p>
		{/if}
	</header>

	{#if files.length === 0}
		<button
			type="button"
			onclick={selectPdfFiles}
			class="w-full rounded-2xl border border-dashed border-slate-300 bg-slate-50 p-10 text-center hover:cursor-pointer hover:bg-slate-100"
		>
			<p class="text-slate-700">Aucun fichier chargé pour le moment.</p>
		</button>
	{/if}

	{#if selectedPages.length > 0}
		<section class="rounded-2xl border border-slate-200 bg-white p-4 shadow-sm">
			<div class="mb-3 flex items-center justify-between">
				<h2 class="text-lg font-semibold text-slate-900">Pages sélectionnées pour le split</h2>
				<div class="flex items-center gap-3">
					<p class="text-sm text-slate-600">{selectedPages.length} page(s)</p>
					<button
						type="button"
						onclick={() => (selectedPages = [])}
						aria-label="Tout déselectionner"
						class="rounded-md border border-slate-300 px-2 py-1 text-sm font-medium text-slate-700 hover:cursor-pointer hover:bg-slate-100"
					>
						Tout déselectionner
					</button>
				</div>
			</div>
			<ul class="grid grid-cols-1 gap-3 md:grid-cols-2 xl:grid-cols-3">
				{#each selectedPages as page, index (page.pageNumber)}
					<li
						animate:flip={{ duration: 220, easing: cubicOut }}
						class="rounded-xl border border-slate-200 p-3"
					>
						<div class="mb-2 flex items-center justify-between">
							<p class="text-sm font-semibold text-slate-800">
								Page {page.pageNumber} - {normalizeRotation(page.rotation)}°
							</p>
							<button
								type="button"
								onclick={() => selectPage(page.pageNumber)}
								class="rounded-md border border-slate-300 px-2 py-1 text-xs font-medium text-slate-700 hover:cursor-pointer hover:bg-slate-100"
							>
								Retirer
							</button>
						</div>
						<div class="mb-3 flex flex-wrap gap-2">
							<button
								type="button"
								onclick={() => movePage(page.pageNumber, -1)}
								aria-label={`Déplacer la page ${page.pageNumber} vers la gauche`}
								hidden={index === 0}
								class="rounded-md border border-slate-300 px-2 py-1 text-sm font-medium text-slate-700 hover:cursor-pointer hover:bg-slate-100"
							>
								&larr;
							</button>
							<button
								type="button"
								onclick={() => movePage(page.pageNumber, 1)}
								aria-label={`Déplacer la page ${page.pageNumber} vers la droite`}
								hidden={index === selectedPages.length - 1}
								class="rounded-md border border-slate-300 px-2 py-1 text-sm font-medium text-slate-700 hover:cursor-pointer hover:bg-slate-100"
							>
								&rarr;
							</button>
							<button
								type="button"
								onclick={() => rotatePage(page.pageNumber, -90)}
								aria-label={`Tourner la page ${page.pageNumber} vers la gauche`}
								class="rounded-md border border-slate-300 px-2 py-1 text-sm font-medium text-slate-700 hover:cursor-pointer hover:bg-slate-100"
							>
								↺
							</button>
							<button
								type="button"
								onclick={() => rotatePage(page.pageNumber, 90)}
								aria-label={`Tourner la page ${page.pageNumber} vers la droite`}
								class="rounded-md border border-slate-300 px-2 py-1 text-sm font-medium text-slate-700 hover:cursor-pointer hover:bg-slate-100"
							>
								↻
							</button>
						</div>
						{#if files.length === 1 && thumbnails[thumbnailKey(files[0].id, page.pageNumber)]}
							<div
								class="flex h-36 w-full items-center justify-center overflow-hidden rounded-lg border border-slate-200 bg-slate-50"
							>
								<img
									src={thumbnails[thumbnailKey(files[0].id, page.pageNumber)]}
									alt={'Miniature page ' + page.pageNumber}
									class="h-28 w-auto rounded border border-slate-200"
									style:transform={'rotate(' + page.rotation + 'deg)'}
									style:transform-origin="center center"
									style:transition="transform 180ms ease"
								/>
							</div>
						{/if}
					</li>
				{/each}
			</ul>
		</section>
	{/if}

	{#if files.length > 0}
		<section class="rounded-2xl border border-slate-200 bg-white p-4 shadow-sm">
			{#if files.length === 1}
				<h2 class="mb-3 text-lg font-semibold text-slate-900">Fichier chargé</h2>
			{:else}
				<h2 class="mb-3 text-lg font-semibold text-slate-900">Fichiers chargés</h2>
			{/if}

			<ul class="space-y-3">
				{#each files as file (file.id)}
					<li
						animate:flip={{ duration: 220, easing: cubicOut }}
						class="rounded-xl border border-slate-200 p-3"
					>
						<div class="flex flex-wrap items-center justify-between gap-2">
							<div>
								<p class="font-semibold text-slate-900">{file.name}</p>
								<p class="text-sm text-slate-600">
									{formatBytes(file.size)} - {file.nbPages} page(s)
								</p>
							</div>
							<div class="flex flex-wrap gap-2">
								{#if files.length === 1}
									{#if selectedPages.length === file.nbPages}
										<button
											type="button"
											disabled
											aria-label="Toutes les pages sont sélectionnées"
											class="cursor-not-allowed rounded-md border border-emerald-400 bg-emerald-50 px-2 py-1 text-sm font-medium text-emerald-700"
										>
											✓ Tout sélectionné
										</button>
									{:else}
										<button
											type="button"
											onclick={selectAllPages}
											aria-label="Sélectionner toutes les pages"
											class="rounded-md border border-slate-300 px-2 py-1 text-sm font-medium text-slate-700 hover:cursor-pointer hover:bg-slate-100"
										>
											Tout sélectionner
										</button>
									{/if}
								{/if}
								<button
									type="button"
									onclick={() => removeFile(file.id)}
									aria-label={`Supprimer ${file.name}`}
									class="rounded-md border border-rose-300 bg-rose-50 px-2 py-1 text-xs font-semibold text-rose-700 transition hover:cursor-pointer hover:bg-rose-100"
								>
									Supprimer
								</button>
								<button
									type="button"
									onclick={() => moveFile(file.id, -1)}
									aria-label={`Monter ${file.name}`}
									hidden={files[0]?.id === file.id}
									class="rounded-md border border-slate-300 px-2 py-1 text-sm font-medium text-slate-700 hover:cursor-pointer hover:bg-slate-100"
								>
									&uarr;
								</button>
								<button
									type="button"
									onclick={() => moveFile(file.id, 1)}
									aria-label={`Descendre ${file.name}`}
									hidden={files[files.length - 1]?.id === file.id}
									class="rounded-md border border-slate-300 px-2 py-1 text-sm font-medium text-slate-700 hover:cursor-pointer hover:bg-slate-100"
								>
									&darr;
								</button>
							</div>
						</div>

						{#if files.length > 1 && thumbnails[thumbnailKey(file.id, 1)]}
							<img
								src={thumbnails[thumbnailKey(file.id, 1)]}
								alt={`Miniature page 1 - ${file.name}`}
								class="mt-3 h-24 w-auto rounded-md border border-slate-200"
							/>
						{/if}

						{#if files.length === 1}
							<ul class="mt-3 grid grid-cols-1 gap-2 sm:grid-cols-2 lg:grid-cols-3">
								{#each Array.from({ length: file.nbPages }, (_, i) => i + 1) as pageNumber (pageNumber)}
									<li class="rounded-lg border border-slate-200 p-2">
										<div class="mb-2 flex items-center justify-between">
											<p class="text-sm font-medium text-slate-800">Page {pageNumber}</p>
											{#if selectedPages.some((p) => p.pageNumber === pageNumber)}
												<button
													type="button"
													class="rounded-md border border-emerald-400 bg-emerald-50 px-2 py-1 text-xs font-medium text-emerald-700 hover:cursor-pointer"
													onclick={() => selectPage(pageNumber)}
												>
													Sélectionnée
												</button>
											{:else}
												<button
													type="button"
													class="rounded-md border border-slate-300 px-2 py-1 text-xs font-medium text-slate-700 hover:cursor-pointer hover:bg-slate-100"
													onclick={() => selectPage(pageNumber)}
												>
													Sélectionner
												</button>
											{/if}
										</div>
										{#if thumbnails[thumbnailKey(file.id, pageNumber)]}
											<img
												src={thumbnails[thumbnailKey(file.id, pageNumber)]}
												alt={`Miniature page ${pageNumber}`}
												class="h-24 w-auto rounded-md border border-slate-200"
											/>
										{/if}
									</li>
								{/each}
							</ul>
						{/if}
					</li>
				{/each}
			</ul>
		</section>
	{/if}
</main>
