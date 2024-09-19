use cudarc::driver::{DevicePtrMut, DeviceRepr, DeviceSlice};
use del_canvas_cuda::pnt2xyzrgb::Splat;
use del_canvas_cuda::pnt2xyzrgb::XyzRgb;
use num_traits::AsPrimitive;
fn main() -> anyhow::Result<()> {
    // let path = "/Users/nobuyuki/project/juice_box1.ply";
    let file_path = "C:/Users/nobuy/Downloads/juice_box.ply";
    let pnt2xyzrgb = del_msh_core::io_ply::read_xyzrgb::<_, XyzRgb>(file_path)?;
    let aabb3 = del_msh_core::vtx2xyz::aabb3_from_points(&pnt2xyzrgb);
    let aabb3: [f32; 6] = aabb3.map(|v| v.as_());
    let img_shape = (2000usize, 1200usize);
    let transform_world2ndc = {
        let cam_proj = del_geo_core::mat4_col_major::camera_perspective_blender(
            img_shape.0 as f32 / img_shape.1 as f32,
            50f32,
            0.1,
            2.0,
            true,
        );
        let cam_modelview = del_geo_core::mat4_col_major::camera_external_blender(
            &[
                (aabb3[0] + aabb3[3]) * 0.5f32,
                (aabb3[1] + aabb3[4]) * 0.5f32,
                (aabb3[2] + aabb3[5]) * 0.5f32 + 1.4f32,
            ],
            0f32,
            0f32,
            0f32,
        );
        del_geo_core::mat4_col_major::mult_mat(&cam_proj, &cam_modelview)
    };
    let radius = 0.0015f32;

    let dev = cudarc::driver::CudaDevice::new(0)?;
    //
    let pnt2xyzrgb_dev = dev.htod_copy(pnt2xyzrgb.clone())?;
    let mut pnt2splat_dev = {
        let pnt2splat = vec![Splat::default(); pnt2xyzrgb.len()];
        dev.htod_copy(pnt2splat.clone())?
    };
    let transform_world2ndc_dev = dev.htod_copy(transform_world2ndc.to_vec())?;
    del_canvas_cuda::pnt2xyzrgb::pnt2xyzrgb_to_pnt2splat(
        &dev,
        &pnt2xyzrgb_dev,
        &mut pnt2splat_dev,
        &transform_world2ndc_dev,
        (img_shape.0 as u32, img_shape.1 as u32),
        radius,
    )?;
    {
        // draw pixels in cpu using the order computed in cpu
        let pnt2splat = dev.dtoh_sync_copy(&pnt2splat_dev)?;
        let idx2vtx = {
            let mut idx2vtx: Vec<usize> = (0..pnt2splat.len()).collect();
            idx2vtx.sort_by(|&idx0, &idx1| {
                pnt2splat[idx0]
                    .ndc_z
                    .partial_cmp(&pnt2splat[idx1].ndc_z)
                    .unwrap()
            });
            idx2vtx
        };
        let mut img_data = vec![[0f32, 0f32, 0f32]; img_shape.0 * img_shape.1];
        del_canvas_cpu::rasterize_aabb3::wireframe_dda(
            &mut img_data,
            img_shape,
            &transform_world2ndc,
            &aabb3,
            [1.0, 1.0, 1.0],
        );
        for i_idx in 0..pnt2xyzrgb.len() {
            let i_vtx = idx2vtx[i_idx];
            let r0 = pnt2splat[i_vtx].pos_pix;
            let ix = r0[0] as usize;
            let iy = r0[1] as usize;
            // dbg!(ix, iy);
            let ipix = iy * img_shape.0 + ix;
            img_data[ipix][0] = (pnt2xyzrgb[i_vtx].rgb[0] as f32) / 255.0;
            img_data[ipix][1] = (pnt2xyzrgb[i_vtx].rgb[1] as f32) / 255.0;
            img_data[ipix][2] = (pnt2xyzrgb[i_vtx].rgb[2] as f32) / 255.0;
        }
        use ::slice_of_array::SliceFlatExt; // for flat
        del_canvas_cpu::write_png_from_float_image_rgb(
            "../target/ply_pixel_cuda.png",
            &img_shape,
            (&img_data).flat(),
        )?;
    } // end pixel
      // ---------------------------------------------------------
      // draw circles with tiles
    const TILE_SIZE: usize = 16;
    assert_eq!(img_shape.0 % TILE_SIZE, 0);
    assert_eq!(img_shape.0 % TILE_SIZE, 0);
    let tile_shape = (img_shape.0 / TILE_SIZE, img_shape.1 / TILE_SIZE);
    let now = std::time::Instant::now();
    let (tile2idx_dev, idx2pnt_dev) = del_canvas_cuda::pnt2xyzrgb::tile2idx_idx2pnt(
        &dev,
        (tile_shape.0 as u32, tile_shape.1 as u32),
        &pnt2splat_dev,
    )?;
    println!("tile2idx_idx2pnt: {:.2?}", now.elapsed());
    {
        // debug tile2ind using cpu
        let pnt2splat = dev.dtoh_sync_copy(&pnt2splat_dev)?;
        let num_tile = tile_shape.0 * tile_shape.1;
        let mut tile2idx_cpu = vec![0usize; num_tile + 1];
        for i_vtx in 0..pnt2splat.len() {
            let p0 = pnt2splat[i_vtx].pos_pix;
            let rad = pnt2splat[i_vtx].rad_pix;
            let aabb2 = del_geo_core::aabb2::from_point(&p0, rad);
            let tiles = del_geo_core::aabb2::overlapping_tiles(&aabb2, TILE_SIZE, tile_shape);
            for &i_tile in tiles.iter() {
                tile2idx_cpu[i_tile + 1] += 1;
            }
        }
        for i_tile in 0..num_tile {
            tile2idx_cpu[i_tile + 1] += tile2idx_cpu[i_tile];
        }
        let tile2idx = dev.dtoh_sync_copy(&tile2idx_dev)?;
        tile2idx
            .iter()
            .zip(tile2idx_cpu.iter())
            .for_each(|(&a, &b)| {
                assert_eq!(a as usize, b);
            });
    } // end debug tile2ind
    {
        // assert ind2pnt by cpu code
        let num_ind = idx2pnt_dev.len();
        let pnt2splat = dev.dtoh_sync_copy(&pnt2splat_dev)?;
        let idx2pnt_gpu = dev.dtoh_sync_copy(&idx2pnt_dev)?;
        let mut idx2pnt_cpu = vec![0usize; num_ind as usize];
        let mut ind2tiledepth = Vec::<(usize, usize, f32)>::with_capacity(num_ind as usize);
        for i_pnt in 0..pnt2splat.len() {
            let p0 = pnt2splat[i_pnt].pos_pix;
            let rad = pnt2splat[i_pnt].rad_pix;
            let depth = pnt2splat[i_pnt].ndc_z;
            let aabb2 = del_geo_core::aabb2::from_point(&p0, rad);
            let tiles = del_geo_core::aabb2::overlapping_tiles(&aabb2, TILE_SIZE, tile_shape);
            for &i_tile in tiles.iter() {
                idx2pnt_cpu[ind2tiledepth.len()] = i_pnt;
                ind2tiledepth.push((i_pnt, i_tile, depth));
            }
        }
        assert_eq!(ind2tiledepth.len(), num_ind as usize);
        ind2tiledepth.sort_by(|&a, &b| {
            if a.1 == b.1 {
                a.2.partial_cmp(&b.2).unwrap()
            } else {
                a.1.cmp(&b.1)
            }
        });
        for iind in 0..ind2tiledepth.len() {
            idx2pnt_cpu[iind] = ind2tiledepth[iind].0;
        }
        assert_eq!(idx2pnt_cpu.len(), idx2pnt_gpu.len());
        idx2pnt_cpu
            .iter()
            .zip(idx2pnt_gpu.iter())
            .for_each(|(&ipnt0, &ipnt1)| {
                assert_eq!(ipnt0, ipnt1 as usize);
            })
    } // assert "idx2pnt" using cpu
    {
        let mut pix2rgb_dev = dev.alloc_zeros::<f32>(img_shape.0 * img_shape.1 * 3)?;
        let now = std::time::Instant::now();
        del_canvas_cuda::pnt2xyzrgb::splat(
            &dev,
            (img_shape.0 as u32, img_shape.1 as u32),
            &mut pix2rgb_dev,
            &pnt2splat_dev,
            TILE_SIZE as u32,
            &tile2idx_dev,
            &idx2pnt_dev,
        )?;
        println!("splat: {:.2?}", now.elapsed());
        let pix2rgb = dev.dtoh_sync_copy(&pix2rgb_dev)?;
        del_canvas_cpu::write_png_from_float_image_rgb(
            "../target/ply_cuda_circle_tile.png",
            &img_shape,
            &pix2rgb,
        )?;
    }
    {
        // assert using cpu
        let idx2pnt = dev.dtoh_sync_copy(&idx2pnt_dev)?;
        let pnt2splat = dev.dtoh_sync_copy(&pnt2splat_dev)?;
        let tile2idx = dev.dtoh_sync_copy(&tile2idx_dev)?;
        let mut img_data = vec![[0f32, 0f32, 0f32]; img_shape.0 * img_shape.1];
        del_canvas_cpu::rasterize_aabb3::wireframe_dda(
            &mut img_data,
            img_shape,
            &transform_world2ndc,
            &aabb3,
            [1.0, 1.0, 1.0],
        );
        for (iw, ih) in itertools::iproduct!(0..img_shape.0, 0..img_shape.1) {
            let i_tile = (ih / TILE_SIZE) * tile_shape.0 + (iw / TILE_SIZE);
            let i_pix = ih * img_shape.0 + iw;
            for &i_vtx in &idx2pnt[tile2idx[i_tile] as usize..tile2idx[i_tile + 1] as usize] {
                let i_vtx = i_vtx as usize;
                let p0 = pnt2splat[i_vtx].pos_pix;
                let rad = pnt2splat[i_vtx].rad_pix;
                let p1 = [iw as f32 + 0.5f32, ih as f32 + 0.5f32];
                if del_geo_core::edge2::length(&p0, &p1) > rad {
                    continue;
                }
                img_data[i_pix][0] = (pnt2xyzrgb[i_vtx].rgb[0] as f32) / 255.0;
                img_data[i_pix][1] = (pnt2xyzrgb[i_vtx].rgb[1] as f32) / 255.0;
                img_data[i_pix][2] = (pnt2xyzrgb[i_vtx].rgb[2] as f32) / 255.0;
            }
        }
        use ::slice_of_array::SliceFlatExt; // for flat
        del_canvas_cpu::write_png_from_float_image_rgb(
            "../target/ply_cuda_circle_tile_cpu_rasterization.png",
            &img_shape,
            (&img_data).flat(),
        )?;
    }

    Ok(())
}