//! Module containing primitives pertaining to the [`LWE programmable
//! bootstrap`](`LweBootstrapKey#programmable-bootstrapping`).

use crate::core_crypto::commons::computation_buffers::ComputationBuffers;
use crate::core_crypto::commons::parameters::*;
use crate::core_crypto::commons::traits::*;
use crate::core_crypto::entities::*;
use crate::core_crypto::fft_impl::crypto::bootstrap::{bootstrap_scratch, FourierLweBootstrapKey};
use crate::core_crypto::fft_impl::crypto::wop_pbs::blind_rotate_assign_scratch;
use crate::core_crypto::fft_impl::math::fft::{Fft, FftView};
use concrete_fft::c64;
use dyn_stack::{DynStack, SizeOverflow, StackReq};

/// Perform a blind rotation given an input [`LWE ciphertext`](`LweCiphertext`), modifying a look-up
/// table passed as a [`GLWE ciphertext`](`GlweCiphertext`) and an [`LWE bootstrap
/// key`](`LweBootstrapKey`) in the fourier domain.
///
/// If you want to manage the computation memory manually you can use
/// [`blind_rotate_assign_mem_optimized`].
pub fn blind_rotate_assign<Scalar, InputCont, OutputCont, KeyCont>(
    input: &LweCiphertext<InputCont>,
    lut: &mut GlweCiphertext<OutputCont>,
    fourier_bsk: &FourierLweBootstrapKey<KeyCont>,
) where
    // CastInto required for PBS modulus switch which returns a usize
    Scalar: UnsignedTorus + CastInto<usize>,
    InputCont: Container<Element = Scalar>,
    OutputCont: ContainerMut<Element = Scalar>,
    KeyCont: Container<Element = c64>,
{
    let mut buffers = ComputationBuffers::new();

    let fft = Fft::new(fourier_bsk.polynomial_size());
    let fft = fft.as_view();

    buffers.resize(
        blind_rotate_assign_mem_optimized_requirement::<Scalar>(
            fourier_bsk.glwe_size(),
            fourier_bsk.polynomial_size(),
            fft,
        )
        .unwrap()
        .unaligned_bytes_required(),
    );

    let stack = buffers.stack();

    blind_rotate_assign_mem_optimized(input, lut, fourier_bsk, fft, stack);
}

/// Memory optimized version of [`blind_rotate_assign`], the caller must provide
/// a properly configured [`FftView`] object and a `DynStack` used as a memory buffer having a
/// capacity at least as large as the result of [`blind_rotate_assign_mem_optimized_requirement`].
pub fn blind_rotate_assign_mem_optimized<Scalar, InputCont, OutputCont, KeyCont>(
    input: &LweCiphertext<InputCont>,
    lut: &mut GlweCiphertext<OutputCont>,
    fourier_bsk: &FourierLweBootstrapKey<KeyCont>,
    fft: FftView<'_>,
    stack: DynStack<'_>,
) where
    // CastInto required for PBS modulus switch which returns a usize
    Scalar: UnsignedTorus + CastInto<usize>,
    InputCont: Container<Element = Scalar>,
    OutputCont: ContainerMut<Element = Scalar>,
    KeyCont: Container<Element = c64>,
{
    fourier_bsk
        .as_view()
        .blind_rotate_assign(lut.as_mut_view(), input.as_ref(), fft, stack);
}

/// Return the required memory for [`blind_rotate_assign_mem_optimized`].
pub fn blind_rotate_assign_mem_optimized_requirement<Scalar>(
    glwe_size: GlweSize,
    polynomial_size: PolynomialSize,
    fft: FftView<'_>,
) -> Result<StackReq, SizeOverflow> {
    blind_rotate_assign_scratch::<Scalar>(glwe_size, polynomial_size, fft)
}

/// Perform a programmable bootsrap given an input [`LWE ciphertext`](`LweCiphertext`), a
/// look-up table passed as a [`GLWE ciphertext`](`GlweCiphertext`) and an [`LWE bootstrap
/// key`](`LweBootstrapKey`) in the fourier domain. The result is written in the provided output
/// [`LWE ciphertext`](`LweCiphertext`).
///
/// If you want to manage the computation memory manually you can use
/// [`programmable_bootstrap_lwe_ciphertext_mem_optimized`].
///
/// # Example
///
/// ```
/// use tfhe::core_crypto::prelude::*;
///
/// // DISCLAIMER: these toy example parameters are not guaranteed to be secure or yield correct
/// // computations
/// // Define the parameters for a 4 bits message able to hold the doubled 2 bits message
/// let small_lwe_dimension = LweDimension(742);
/// let glwe_dimension = GlweDimension(1);
/// let polynomial_size = PolynomialSize(2048);
/// let lwe_modular_std_dev = StandardDev(0.000007069849454709433);
/// let glwe_modular_std_dev = StandardDev(0.00000000000000029403601535432533);
/// let pbs_base_log = DecompositionBaseLog(23);
/// let pbs_level = DecompositionLevelCount(1);
///
/// // Request the best seeder possible, starting with hardware entropy sources and falling back to
/// // /dev/random on Unix systems if enabled via cargo features
/// let mut boxed_seeder = new_seeder();
/// // Get a mutable reference to the seeder as a trait object from the Box returned by new_seeder
/// let seeder = boxed_seeder.as_mut();
///
/// // Create a generator which uses a CSPRNG to generate secret keys
/// let mut secret_generator =
///     SecretRandomGenerator::<ActivatedRandomGenerator>::new(seeder.seed());
///
/// // Create a generator which uses two CSPRNGs to generate public masks and secret encryption
/// // noise
/// let mut encryption_generator =
///     EncryptionRandomGenerator::<ActivatedRandomGenerator>::new(seeder.seed(), seeder);
///
/// println!("Generating keys...");
///
/// // Generate an LweSecretKey with binary coefficients
/// let small_lwe_sk =
///     LweSecretKey::generate_new_binary(small_lwe_dimension, &mut secret_generator);
///
/// // Generate a GlweSecretKey with binary coefficients
/// let glwe_sk =
///     GlweSecretKey::generate_new_binary(glwe_dimension, polynomial_size, &mut secret_generator);
///
/// // Create a copy of the GlweSecretKey re-interpreted as an LweSecretKey
/// let big_lwe_sk = glwe_sk.clone().into_lwe_secret_key();
///
/// // Generate the seeded bootstrapping key to show how to handle entity decompression,
/// // we use the parallel variant for performance reason
/// let std_bootstrapping_key = par_allocate_and_generate_new_seeded_lwe_bootstrap_key(
///     &small_lwe_sk,
///     &glwe_sk,
///     pbs_base_log,
///     pbs_level,
///     glwe_modular_std_dev,
///     seeder,
/// );
///
/// // We decompress the bootstrapping key
/// let std_bootstrapping_key: LweBootstrapKeyOwned<u64> =
///     std_bootstrapping_key.decompress_into_lwe_bootstrap_key();
///
/// // Create the empty bootstrapping key in the Fourier domain
/// let mut fourier_bsk = FourierLweBootstrapKey::new(
///     std_bootstrapping_key.input_lwe_dimension(),
///     std_bootstrapping_key.glwe_size(),
///     std_bootstrapping_key.polynomial_size(),
///     std_bootstrapping_key.decomposition_base_log(),
///     std_bootstrapping_key.decomposition_level_count(),
/// );
///
/// // Use the conversion function (a memory optimized version also exists but is more complicated
/// // to use) to convert the standard bootstrapping key to the Fourier domain
/// convert_standard_lwe_bootstrap_key_to_fourier(&std_bootstrapping_key, &mut fourier_bsk);
/// // We don't need the standard bootstrapping key anymore
/// drop(std_bootstrapping_key);
///
/// // Our 4 bits message space
/// let message_modulus = 1u64 << 4;
///
/// // Our input message
/// let input_message = 3u64;
///
/// // Delta used to encode 4 bits of message + a bit of padding on u64
/// let delta = (1_u64 << 63) / message_modulus;
///
/// // Apply our encoding
/// let plaintext = Plaintext(input_message * delta);
///
/// // Allocate a new LweCiphertext and encrypt our plaintext
/// let lwe_ciphertext_in: LweCiphertextOwned<u64> = allocate_and_encrypt_new_lwe_ciphertext(
///     &small_lwe_sk,
///     plaintext,
///     lwe_modular_std_dev,
///     &mut encryption_generator,
/// );
///
/// // Now we will use a PBS to compute a multiplication by 2, it is NOT the recommended way of
/// // doing this operation in terms of performance as it's much more costly than a multiplication
/// // with a cleartext, however it resets the noise in a ciphertext to a nominal level and allows
/// // to evaluate arbitrary functions so depending on your use case it can be a better fit.
///
/// // Here we will define a helper function to generate an accumulator for a PBS
/// fn generate_accumulator<F>(
///     polynomial_size: PolynomialSize,
///     glwe_size: GlweSize,
///     message_modulus: usize,
///     delta: u64,
///     f: F,
/// ) -> GlweCiphertextOwned<u64>
/// where
///     F: Fn(u64) -> u64,
/// {
///     // N/(p/2) = size of each block, to correct noise from the input we introduce the notion of
///     // box, which manages redundancy to yield a denoised value for several noisy values around
///     // a true input value.
///     let box_size = polynomial_size.0 / message_modulus;
///
///     // Create the accumulator
///     let mut accumulator_u64 = vec![0_u64; polynomial_size.0];
///
///     // Fill each box with the encoded denoised value
///     for i in 0..message_modulus {
///         let index = i * box_size;
///         accumulator_u64[index..index + box_size]
///             .iter_mut()
///             .for_each(|a| *a = f(i as u64) * delta);
///     }
///
///     let half_box_size = box_size / 2;
///
///     // Negate the first half_box_size coefficients to manage negacyclicity and rotate
///     for a_i in accumulator_u64[0..half_box_size].iter_mut() {
///         *a_i = (*a_i).wrapping_neg();
///     }
///
///     // Rotate the accumulator
///     accumulator_u64.rotate_left(half_box_size);
///
///     let accumulator_plaintext = PlaintextList::from_container(accumulator_u64);
///
///     let accumulator =
///         allocate_and_trivially_encrypt_new_glwe_ciphertext(glwe_size, &accumulator_plaintext);
///
///     accumulator
/// }
///
/// // Generate the accumulator for our multiplication by 2 using a simple closure
/// let accumulator: GlweCiphertextOwned<u64> = generate_accumulator(
///     polynomial_size,
///     glwe_dimension.to_glwe_size(),
///     message_modulus as usize,
///     delta,
///     |x: u64| 2 * x,
/// );
///
/// // Allocate the LweCiphertext to store the result of the PBS
/// let mut pbs_multiplication_ct =
///     LweCiphertext::new(0u64, big_lwe_sk.lwe_dimension().to_lwe_size());
/// println!("Computing PBS...");
/// programmable_bootstrap_lwe_ciphertext(
///     &lwe_ciphertext_in,
///     &mut pbs_multiplication_ct,
///     &accumulator,
///     &fourier_bsk,
/// );
///
/// // Decrypt the PBS multiplication result
/// let pbs_multipliation_plaintext: Plaintext<u64> =
///     decrypt_lwe_ciphertext(&big_lwe_sk, &pbs_multiplication_ct);
///
/// /// // Create a SignedDecomposer to perform the rounding of the decrypted plaintext
/// // We pass a DecompositionBaseLog of 5 and a DecompositionLevelCount of 1 indicating we want to
/// // round the 5 MSB, 1 bit of padding plus our 4 bits of message
/// let signed_decomposer =
///     SignedDecomposer::new(DecompositionBaseLog(5), DecompositionLevelCount(1));
///
/// // Round and remove our encoding
/// let pbs_multiplication_result: u64 =
///     signed_decomposer.closest_representable(pbs_multipliation_plaintext.0) / delta;
///
/// println!("Checking result...");
/// assert_eq!(6, pbs_multiplication_result);
/// println!(
///     "Mulitplication via PBS result is correct! Expected 6, got {pbs_multiplication_result}"
/// );
/// ```
pub fn programmable_bootstrap_lwe_ciphertext<Scalar, InputCont, OutputCont, AccCont, KeyCont>(
    input: &LweCiphertext<InputCont>,
    output: &mut LweCiphertext<OutputCont>,
    accumulator: &GlweCiphertext<AccCont>,
    fourier_bsk: &FourierLweBootstrapKey<KeyCont>,
) where
    // CastInto required for PBS modulus switch which returns a usize
    Scalar: UnsignedTorus + CastInto<usize>,
    InputCont: Container<Element = Scalar>,
    OutputCont: ContainerMut<Element = Scalar>,
    AccCont: Container<Element = Scalar>,
    KeyCont: Container<Element = c64>,
{
    let mut buffers = ComputationBuffers::new();

    let fft = Fft::new(fourier_bsk.polynomial_size());
    let fft = fft.as_view();

    buffers.resize(
        programmable_bootstrap_lwe_ciphertext_mem_optimized_requirement::<Scalar>(
            fourier_bsk.glwe_size(),
            fourier_bsk.polynomial_size(),
            fft,
        )
        .unwrap()
        .unaligned_bytes_required(),
    );

    let stack = buffers.stack();

    programmable_bootstrap_lwe_ciphertext_mem_optimized(
        input,
        output,
        accumulator,
        fourier_bsk,
        fft,
        stack,
    )
}

/// Memory optimized version of [`programmable_bootstrap_lwe_ciphertext`], the caller must provide
/// a properly configured [`FftView`] object and a `DynStack` used as a memory buffer having a
/// capacity at least as large as the result of
/// [`programmable_bootstrap_lwe_ciphertext_mem_optimized_requirement`].
pub fn programmable_bootstrap_lwe_ciphertext_mem_optimized<
    Scalar,
    InputCont,
    OutputCont,
    AccCont,
    KeyCont,
>(
    input: &LweCiphertext<InputCont>,
    output: &mut LweCiphertext<OutputCont>,
    accumulator: &GlweCiphertext<AccCont>,
    fourier_bsk: &FourierLweBootstrapKey<KeyCont>,
    fft: FftView<'_>,
    stack: DynStack<'_>,
) where
    // CastInto required for PBS modulus switch which returns a usize
    Scalar: UnsignedTorus + CastInto<usize>,
    InputCont: Container<Element = Scalar>,
    OutputCont: ContainerMut<Element = Scalar>,
    AccCont: Container<Element = Scalar>,
    KeyCont: Container<Element = c64>,
{
    fourier_bsk.as_view().bootstrap(
        output.as_mut(),
        input.as_ref(),
        accumulator.as_view(),
        fft,
        stack,
    );
}

/// Return the required memory for [`programmable_bootstrap_lwe_ciphertext_mem_optimized`].
pub fn programmable_bootstrap_lwe_ciphertext_mem_optimized_requirement<Scalar>(
    glwe_size: GlweSize,
    polynomial_size: PolynomialSize,
    fft: FftView<'_>,
) -> Result<StackReq, SizeOverflow> {
    bootstrap_scratch::<Scalar>(glwe_size, polynomial_size, fft)
}
